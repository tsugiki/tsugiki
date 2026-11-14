use super::*;
use crate::specta_builder;
use crate::tests::{cpython, no_folders, temp_folders};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, Listener, WebviewWindow};
use tsugiki_core::github::CommitSha;
use tsugiki_core::github::issue_content::{Comment, IssueContent};
use tsugiki_core::github::pull_request_content::{
    Anchor, DiffHunk, Opening, PullRequestContent, Resolution, Review, Side, Subject, Submission,
    ThreadComment, ThreadId, ThreadLine,
};
use tsugiki_core::github::pull_requests::{PrState, PullRequestFacts, ReviewState};
use tsugiki_core::github::replay::Replayer;
use tsugiki_core::github::{Reply, Request, Transport, Unreachable};
use tsugiki_core::login::Login;
use tsugiki_core::mirror::MirrorSource;
use tsugiki_core::repository::{RepositoryAccess, RepositoryName};
use tsugiki_core::repository_settings::RepositorySettings;
use tsugiki_core::settings::SettingsFiles;
use tsugiki_core::token::{GhOutput, Token};
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_test_git::git;

/// Longer than any of these tests should wait on a working app.
const PLENTY: Duration = Duration::from_secs(60);

fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: "gho_example".to_owned(),
    })
}

/// Real GitHub's answers, recorded for the core's fixtures.
fn recordings() -> GitHubAccess {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/github");
    GitHubAccess {
        transport: Box::new(Replayer::new(dir)),
        run_gh: signed_in_gh,
    }
}

struct NoNetwork;

impl Transport for NoNetwork {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        Err(Unreachable::Offline)
    }
}

fn no_network() -> GitHubAccess {
    GitHubAccess {
        transport: Box::new(NoNetwork),
        run_gh: signed_in_gh,
    }
}

/// Says when a request arrives, then holds it until the test lets go. A regression then shows up as
/// a slow test that still ends.
struct StuckGitHub {
    arrived: Sender<()>,
    released: Receiver<()>,
}

impl Transport for StuckGitHub {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        self.arrived.send(()).unwrap_or(());
        self.released.recv_timeout(PLENTY).unwrap_or(());
        Err(Unreachable::Offline)
    }
}

fn releases_offline() -> Result<tsugiki_token::Reply, tsugiki_token::Unreachable> {
    Err(tsugiki_token::Unreachable::Offline(
        "no network in tests".to_owned(),
    ))
}

/// GitHub Releases with `tag` as the latest release.
fn latest_release(tag: &str) -> Result<tsugiki_token::Reply, tsugiki_token::Unreachable> {
    Ok(tsugiki_token::Reply {
        status: 200,
        body: json!({ "tag_name": tag }).to_string(),
        date: None,
        ratelimit_reset: None,
        etag: None,
    })
}

/// The app as `startup.rs` wires it, on Tauri's mock runtime with a webview to send commands from. It
/// listens before the session starts, so it hears every event.
struct Wired {
    core: Core,
    webview: WebviewWindow<MockRuntime>,
    statuses: Receiver<Value>,
    workbenches: Receiver<Value>,
    failed_reads: Receiver<Value>,
    last_syncs: Receiver<Value>,
    activities: Receiver<Value>,
    clonings: Receiver<Value>,
    problems: Receiver<Value>,
    questions: Receiver<Value>,
    states: Receiver<Value>,
    held: Receiver<Value>,
    heads: Receiver<Value>,
    failed_heads: Receiver<Value>,
    new_versions: Receiver<Value>,
    _app: App<MockRuntime>,
}

impl Wired {
    /// The app with mirrors cloned from a folder with nothing in it, so each clone fails at once.
    fn start(folders: Result<Folders, Error>, access: GitHubAccess) -> Self {
        Self::start_cloning_from(folders, access, Remotes::under("no-remotes/".to_owned()))
    }

    fn start_cloning_from(
        folders: Result<Folders, Error>,
        github: GitHubAccess,
        remotes: Remotes,
    ) -> Self {
        Self::start_picking(folders, github, remotes, Arc::new(|| None))
    }

    /// The app whose folder picker gives what `pick_folder` gives.
    fn start_picking(
        folders: Result<Folders, Error>,
        github: GitHubAccess,
        remotes: Remotes,
        pick_folder: PickFolder,
    ) -> Self {
        let outside = Outside {
            github,
            releases: Box::new(|_| releases_offline()),
            remotes,
            pick_folder,
        };
        Self::start_outside(folders, outside)
    }

    fn start_outside(folders: Result<Folders, Error>, outside: Outside) -> Self {
        let builder = specta_builder();
        let app = mock_builder()
            .invoke_handler(builder.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        builder.mount_events(&app);
        let statuses = hear(&app, "status-changed");
        let workbenches = hear(&app, "changed");
        let failed_reads = hear(&app, "read-failed");
        let last_syncs = hear(&app, "last-sync-changed");
        let activities = hear(&app, "sync-activity-changed");
        let clonings = hear(&app, "cloning-changed");
        let problems = hear(&app, "mirror-problem-changed");
        let questions = hear(&app, "mirror-question-changed");
        let states = hear(&app, "mirror-states-changed");
        let held = hear(&app, "mirrors-held-changed");
        let heads = hear(&app, "heads-fetched");
        let failed_heads = hear(&app, "heads-fetch-failed");
        let new_versions = hear(&app, "new-version-found");
        // The window opens before the core connects, as `startup.rs` opens it.
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let core = connect_core(app.handle(), folders, outside, Duration::from_secs(3600));
        Self {
            core,
            webview,
            statuses,
            workbenches,
            failed_reads,
            last_syncs,
            activities,
            clonings,
            problems,
            questions,
            states,
            held,
            heads,
            failed_heads,
            new_versions,
            _app: app,
        }
    }

    /// The webview's answer to `command`, through Tauri's own handling of its arguments and threads.
    #[expect(
        clippy::unwrap_in_result,
        reason = "the `Result` is the command's answer, and anything else should fail the test"
    )]
    fn ask(&self, command: &str, arguments: Value) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            &self.webview,
            tauri::webview::InvokeRequest {
                cmd: command.to_owned(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: self.webview.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(arguments),
                headers: tauri::http::HeaderMap::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .map(|answer| answer.deserialize().unwrap())
    }

    fn stop(self) -> Self {
        self.core.stop(PLENTY);
        self
    }
}

/// Each payload of `event`, as the webview would get it.
fn hear(app: &App<MockRuntime>, event: &str) -> Receiver<Value> {
    let (heard, payloads) = channel();
    let _ = app.listen(event, move |event| {
        drop(heard.send(serde_json::from_str(event.payload()).unwrap()));
    });
    payloads
}

const CPYTHON: &str = "python/cpython";

fn add_cpython() -> Value {
    json!({ "repository": { "name": CPYTHON, "access": "other", "source": { "kind": "download" } } })
}

fn the_whole_workbench() -> Value {
    json!({ "repository": CPYTHON, "view": { "kind": "whole" } })
}

#[test]
fn with_no_folders_every_command_that_needs_them_says_so_in_the_cores_form() {
    let wired = Wired::start(Err(no_folders()), no_network());

    let checking = wired.ask("check_repository", json!({ "input": "a/b" }));
    let adding = wired.ask("add_repository", add_cpython());
    let reading = wired.ask(
        "workbench",
        json!({ "repository": "a/b", "view": { "kind": "whole" } }),
    );
    let files = wired.ask(
        "pull_request_files",
        json!({ "repository": "a/b", "number": 1 }),
    );
    let layout = wired.ask("save_pane_layout", json!({ "layout": { "sidebar": 240 } }));
    drop(wired.stop());

    let checking = checking.unwrap_err();
    assert_eq!(checking["kind"], "offline");
    assert!(
        checking["message"]
            .as_str()
            .unwrap()
            .contains("isn't connected")
    );
    let adding = adding.unwrap_err();
    assert_eq!(adding["kind"], "internal");
    assert_eq!(adding["message"], no_folders().message);
    assert_eq!(reading.unwrap_err()["kind"], "internal");
    assert_eq!(files.unwrap_err()["message"], no_folders().message);
    assert_eq!(layout.unwrap_err()["message"], no_folders().message);
}

#[test]
fn a_development_build_opens_devtools_for_the_window_that_asks() {
    let wired = Wired::start(Err(no_folders()), no_network());

    let about = wired.ask("about", json!({}));
    let opened = wired.ask("open_devtools", json!({}));
    drop(wired.stop());

    assert_eq!(about.unwrap()["build"], "development");
    assert_eq!(opened, Ok(Value::Null));
}

#[test]
fn a_name_that_isnt_one_is_refused_before_the_missing_folders_matter() {
    let wired = Wired::start(Err(no_folders()), no_network());
    let nonsense =
        json!({ "name": "nonsense", "access": "other", "source": { "kind": "download" } });

    let adding = wired.ask("add_repository", json!({ "repository": nonsense }));
    let checking = wired.ask("check_repository", json!({ "input": "nonsense" }));
    let costing = wired.ask("mirror_cost", json!({ "repository": "nonsense" }));
    let reading = wired.ask(
        "workbench",
        json!({ "repository": "nonsense", "view": { "kind": "whole" } }),
    );
    drop(wired.stop());

    assert_eq!(adding.unwrap_err()["kind"], "not_found");
    assert_eq!(checking.unwrap_err()["kind"], "not_found");
    assert_eq!(costing.unwrap_err()["kind"], "not_found");
    assert_eq!(reading.unwrap_err()["kind"], "not_found");
}

#[test]
fn the_files_tabs_check_of_a_mirrors_cost_goes_to_the_session_and_back() {
    let (folders, root) = temp_folders("wired-cost");
    let wired = Wired::start(Ok(folders), no_network());

    let answer = wired.ask("mirror_cost", json!({ "repository": CPYTHON }));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let answer = answer.unwrap_err();
    assert_eq!(answer["kind"], "offline");
    assert_eq!(answer["message"], "Tsugiki can't reach GitHub.");
}

#[test]
fn the_add_dialogs_check_goes_to_the_session_and_back() {
    let (folders, root) = temp_folders("wired-check");
    let wired = Wired::start(Ok(folders), no_network());

    let answer = wired.ask("check_repository", json!({ "input": CPYTHON }));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let answer = answer.unwrap_err();
    assert_eq!(answer["kind"], "offline");
    assert_eq!(answer["message"], "Tsugiki can't reach GitHub.");
}

#[test]
fn a_newer_version_reaches_the_window_as_an_event_and_through_its_command() {
    let (folders, root) = temp_folders("wired-new-version");
    let outside = Outside {
        github: no_network(),
        releases: Box::new(|_| latest_release("v999.0.0")),
        remotes: Remotes::under("no-remotes/".to_owned()),
        pick_folder: Arc::new(|| None),
    };
    let wired = Wired::start_outside(Ok(folders), outside);

    let heard = wired.new_versions.recv_timeout(PLENTY).unwrap();
    let asked = wired.ask("new_version", json!({}));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let expected = json!({
        "version": "999.0.0",
        "page": "https://github.com/tsugiki/tsugiki/releases/tag/v999.0.0",
    });
    assert_eq!(heard, expected);
    assert_eq!(asked.unwrap(), expected);
}

#[test]
fn with_the_check_off_tsugiki_sends_releases_nothing() {
    assert_releases_hear_nothing("wired-no-check", "[releases]\ncheck = false\n");
}

#[test]
fn settings_tsugiki_cant_read_send_releases_nothing_as_they_may_turn_the_check_off() {
    assert_releases_hear_nothing("wired-unread-check", "[releases]\ncheck = \"no\"\n");
}

/// Starts the app with `settings` as its `settings.toml`, and checks that it neither asks Releases nor
/// says that a version is out.
fn assert_releases_hear_nothing(name: &str, settings: &str) {
    let (folders, root) = temp_folders(name);
    std::fs::write(folders.settings_file(), settings).unwrap();
    let (asking, asks) = channel();
    let outside = Outside {
        github: no_network(),
        releases: Box::new(move |_| {
            asking.send(()).unwrap_or(());
            latest_release("v999.0.0")
        }),
        remotes: Remotes::under("no-remotes/".to_owned()),
        pick_folder: Arc::new(|| None),
    };
    let wired = Wired::start_outside(Ok(folders), outside);

    // Without a check, nothing keeps the stand-in for Releases.
    let asked = asks.recv_timeout(PLENTY);
    let answer = wired.ask("new_version", json!({}));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(asked, Err(std::sync::mpsc::RecvTimeoutError::Disconnected));
    assert_eq!(answer.unwrap(), Value::Null);
}

#[test]
fn themes_answers_with_the_mode_and_the_theme_of_each_side_the_settings_chose() {
    let (folders, root) = temp_folders("wired-themes");
    let settings = "[appearance]\ntheme = \"light\"\ndark_theme = \"Gruvbox Dark Hard\"\n";
    std::fs::write(folders.settings_file(), settings).unwrap();
    let wired = Wired::start(Ok(folders), no_network());

    let answer = wired.ask("themes", json!({}));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let answer = answer.unwrap();
    assert_eq!(answer["mode"], "light");
    assert_eq!(answer["light"]["chosen"], "VS Code Light Modern");
    assert_eq!(answer["dark"]["chosen"], "Gruvbox Dark Hard");
    assert_eq!(answer["dark"]["all"][1]["name"], "Gruvbox Dark Hard");
    assert_eq!(answer["dark"]["all"][1]["tokens"][0]["value"], "#1d2021");
}

/// The themes come from the folder beside the settings file in use, read again on each ask, so an edited
/// file shows without a restart (#519).
#[test]
fn themes_reads_the_themes_folder_beside_settings_path_on_each_ask() {
    let (folders, root) = temp_folders("wired-theme-files");
    let dotfiles = root.join("dotfiles");
    std::fs::create_dir_all(dotfiles.join("themes")).unwrap();
    std::fs::write(dotfiles.join("tsugiki.toml"), "").unwrap();
    let path = dotfiles.join("tsugiki.toml");
    let path = path.to_str().unwrap().replace('\\', "/");
    std::fs::write(
        folders.machine_file(),
        format!("settings_path = \"{path}\"\n"),
    )
    .unwrap();
    let night = |bg: &str| format!("name = \"Night\"\nkind = \"dark\"\n[colors]\nbg = \"{bg}\"\n");
    let file = dotfiles.join("themes").join("night.toml");
    std::fs::write(&file, night("#000010")).unwrap();
    let wired = Wired::start(Ok(folders), no_network());

    let first = wired.ask("themes", json!({})).unwrap();
    std::fs::write(&file, night("#000020")).unwrap();
    let saved = wired.ask("save_theme", json!({ "kind": "dark", "name": "Night" }));
    let second = wired.ask("themes", json!({})).unwrap();
    drop(wired.stop());
    let settings = std::fs::read_to_string(dotfiles.join("tsugiki.toml")).unwrap();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(first["dark"]["all"][2]["name"], "Night");
    assert_eq!(first["dark"]["all"][2]["tokens"][0]["value"], "#000010");
    assert_eq!(first["dark"]["chosen"], "VS Code Dark Modern");
    assert_eq!(saved.unwrap(), Value::Null);
    assert_eq!(second["dark"]["all"][2]["tokens"][0]["value"], "#000020");
    assert_eq!(second["dark"]["chosen"], "Night");
    assert_eq!(settings, "[appearance]\ndark_theme = \"Night\"\n");
}

#[test]
fn a_theme_the_picker_saves_is_chosen_after_a_restart() {
    let (folders, root) = temp_folders("wired-pick-theme");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let saved = wired.ask(
        "save_theme",
        json!({ "kind": "dark", "name": "Gruvbox Dark Hard" }),
    );
    let unknown = wired.ask(
        "save_theme",
        json!({ "kind": "light", "name": "Gruvbox Dark Hard" }),
    );
    // A page that loads again, as after a crash of the webview, finds the theme picked.
    let reloaded = wired.ask("themes", json!({})).unwrap();
    drop(wired.stop());

    let restarted = Wired::start(Ok(folders.clone()), no_network());
    let themes = restarted.ask("themes", json!({})).unwrap();
    drop(restarted.stop());
    let file = std::fs::read_to_string(folders.settings_file()).unwrap();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(saved.unwrap(), Value::Null);
    let unknown = unknown.unwrap_err();
    assert_eq!(unknown["kind"], "not_found");
    assert_eq!(
        unknown["message"],
        r#"No light theme is named "Gruvbox Dark Hard"."#
    );
    assert_eq!(reloaded["dark"]["chosen"], "Gruvbox Dark Hard");
    assert_eq!(themes["dark"]["chosen"], "Gruvbox Dark Hard");
    assert_eq!(file, "[appearance]\ndark_theme = \"Gruvbox Dark Hard\"\n");
}

#[test]
fn the_add_dialogs_check_of_a_repository_tsugiki_has_answers_without_github() {
    let (folders, root) = temp_folders("wired-held");
    let wired = Wired::start(Ok(folders), no_network());

    let added = wired.ask("add_repository", add_cpython());
    let answer = wired.ask("check_repository", json!({ "input": "Python/CPython" }));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(added.unwrap()["screen"], "repository");
    assert_eq!(answer.unwrap(), json!({ "kind": "held", "name": CPYTHON }));
}

#[test]
fn an_add_and_a_read_dont_wait_for_github() {
    let (folders, root) = temp_folders("wired-stuck");
    let (arrived, request_arrived) = channel();
    let (release, released) = channel();
    let access = GitHubAccess {
        transport: Box::new(StuckGitHub { arrived, released }),
        run_gh: signed_in_gh,
    };
    let wired = Wired::start(Ok(folders), access);
    request_arrived.recv_timeout(PLENTY).unwrap();

    let started = Instant::now();
    let added = wired.ask("add_repository", add_cpython());
    let read = wired.ask("workbench", the_whole_workbench());
    let waited = started.elapsed();
    drop(release);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(added.unwrap()["screen"], "repository");
    assert_eq!(read.unwrap()["items"], json!([]));
    assert!(waited < PLENTY / 2, "{waited:?}");
}

#[test]
fn the_panes_layout_stays_on_this_machine_across_a_restart() {
    let (folders, root) = temp_folders("wired-panes");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let before = wired.ask("pane_layout", json!({})).unwrap();
    let layout = json!({
        "sidebar": 240,
        "list": null,
        "sidebar_hidden": true,
        "list_hidden": true,
        "detail_only_hid": "both",
        "order": "last_update",
    });
    let saved = wired.ask("save_pane_layout", json!({ "layout": layout }));
    drop(wired.stop());

    let restarted = Wired::start(Ok(folders.clone()), no_network());
    let reopened = restarted.ask("pane_layout", json!({})).unwrap();
    drop(restarted.stop());
    let synced = folders.settings_file().exists();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(
        before,
        json!({
            "sidebar": null,
            "list": null,
            "sidebar_hidden": false,
            "list_hidden": false,
            "detail_only_hid": null,
            "order": "attention",
        })
    );
    assert_eq!(
        (saved.unwrap(), reopened, synced),
        (layout.clone(), layout, false)
    );
}

#[test]
fn a_repository_added_reaches_both_files_and_opens_after_a_restart() {
    let (folders, root) = temp_folders("wired-restart");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let before = wired.ask("home", json!({})).unwrap();
    let added = wired.ask("add_repository", add_cpython()).unwrap();
    let after = wired.ask("home", json!({})).unwrap();
    drop(wired.stop());

    let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
    let machine = std::fs::read_to_string(folders.machine_file()).unwrap();
    let restarted = Wired::start(Ok(folders), no_network());
    let reopened = restarted.ask("home", json!({})).unwrap();
    drop(restarted.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(before["screen"], "onboarding");
    assert_eq!(added, json!(cpython()));
    assert_eq!(after, added);
    assert_eq!(reopened, added);
    assert!(
        settings.contains(r#"name = "python/cpython""#),
        "{settings}"
    );
    assert!(
        settings.contains(r#"conventions = "cpython""#),
        "{settings}"
    );
    assert!(
        machine.contains(r#"{ name = "python/cpython", opened_at = "#),
        "{machine}"
    );
}

#[test]
fn a_switch_opens_the_repository_syncs_it_and_a_restart_reopens_it() {
    let (folders, root) = temp_folders("wired-switch");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let peps =
        json!({ "name": "python/peps", "access": "other", "source": { "kind": "download" } });
    let _ = wired
        .ask("add_repository", json!({ "repository": peps }))
        .unwrap();

    let opened = wired.ask("open_repository", json!({ "name": "Python/CPython" }));
    let home = wired.ask("home", json!({})).unwrap();
    let synced: Vec<Value> = (0..3)
        .map(|_| wired.last_syncs.recv_timeout(PLENTY).unwrap()["repository"].clone())
        .collect();
    let gone = wired.ask("open_repository", json!({ "name": "python/gone" }));
    let nonsense = wired.ask("open_repository", json!({ "name": "nonsense" }));
    drop(wired.stop());
    let restarted = Wired::start(Ok(folders), no_network());
    let reopened = restarted.ask("home", json!({})).unwrap();
    drop(restarted.stop());
    std::fs::remove_dir_all(root).unwrap();

    // The Repository picker lists the one left behind below the open one.
    let mut switched = json!(cpython());
    switched["others"] = json!(["python/peps"]);
    assert_eq!(opened.unwrap(), switched);
    assert_eq!(home, switched);
    // The session tells the status bar when each Repository opened last synced, the switched-to one last.
    assert_eq!(synced, [CPYTHON, "python/peps", CPYTHON]);
    assert_eq!(
        gone.unwrap_err()["message"],
        "Tsugiki doesn't have python/gone."
    );
    assert_eq!(nonsense.unwrap_err()["kind"], "not_found");
    assert_eq!(reopened, switched);
}

#[test]
fn a_window_that_loads_after_an_announcement_gets_the_same_status() {
    let (folders, root) = temp_folders("wired-status");
    let wired = Wired::start(Ok(folders), no_network());

    let announced = wired.statuses.recv_timeout(PLENTY).unwrap();
    let asked = wired.ask("status", json!({})).unwrap();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(announced, json!({ "state": "offline" }));
    assert_eq!(asked, announced);
}

/// A system browser that won't start.
fn no_browser(_: &WebAddress) -> Result<(), Error> {
    Err(Error::new(
        tsugiki_core::error::ErrorKind::Internal,
        "no browser",
    ))
}

#[test]
fn opening_an_issue_moves_nothing_and_mark_unseen_and_mark_seen_tell_the_window() {
    let (folders, root) = temp_folders("wired-open-issue");
    let wired = Wired::start(Ok(folders), no_network());
    let issue_7 = json!({ "repository": CPYTHON, "number": 7 });
    let mark = json!({ "numbers": [7, 8], "up_to": "2026-11-01T09:00:00Z" });

    drop(wired.ask("open_issue", issue_7.clone()).unwrap());
    drop(wired.ask("fetch_issue", issue_7.clone()).unwrap());
    drop(
        wired
            .ask("mark_unseen", json!({ "repository": CPYTHON, "number": 8 }))
            .unwrap(),
    );
    // The session takes its messages in order, so opening would have told the window first.
    let flagged = wired.workbenches.recv_timeout(PLENTY).unwrap();
    drop(
        wired
            .ask("mark_seen", json!({ "repository": CPYTHON, "mark": mark }))
            .unwrap(),
    );
    let seen = wired.workbenches.recv_timeout(PLENTY).unwrap();
    let held = wired.ask("issue", issue_7).unwrap();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(flagged, json!({ "repository": CPYTHON, "items": [8] }));
    assert_eq!(seen, json!({ "repository": CPYTHON, "items": [7, 8] }));
    assert_eq!(held, Value::Null, "nothing has saved an Issue #7");
}

#[test]
fn a_mark_seen_with_a_moment_that_isnt_one_is_refused() {
    let (folders, root) = temp_folders("wired-bad-mark");
    let wired = Wired::start(Ok(folders), no_network());
    let mark = json!({ "numbers": [7], "up_to": "yesterday" });

    let refused = wired.ask("mark_seen", json!({ "repository": CPYTHON, "mark": mark }));
    let bad_name = wired.ask(
        "mark_unseen",
        json!({ "repository": "not a name", "number": 7 }),
    );
    let wired = wired.stop();
    std::fs::remove_dir_all(root).unwrap();

    assert!(refused.is_err());
    assert_eq!(bad_name.unwrap_err()["kind"], "not_found");
    assert!(wired.workbenches.try_recv().is_err());
}

#[test]
fn a_pr_read_github_cant_answer_tells_the_window_it_waits_on_github() {
    let (folders, root) = temp_folders("wired-pr-offline");
    let wired = Wired::start(Ok(folders), no_network());

    drop(
        wired
            .ask(
                "fetch_pull_request",
                json!({ "repository": CPYTHON, "number": 7 }),
            )
            .unwrap(),
    );
    let failed = wired.failed_reads.recv_timeout(PLENTY).unwrap();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(
        failed,
        json!({ "repository": CPYTHON, "number": 7, "why": "waits_on_github" })
    );
}

/// The Issue a Workbench lists with PR `number` in its tree.
fn issue_over(workbench: &Value, number: u64) -> &Value {
    listed(workbench)
        .into_iter()
        .find(|item| {
            let entries = item["pull_requests"].as_array().unwrap();
            entries.iter().any(|entry| entry["number"] == number)
        })
        .unwrap()
}

/// Waits for a `changed` event that names item `number`.
fn wait_for_change_to(changes: &Receiver<Value>, number: &Value) {
    while !changes.recv_timeout(PLENTY).unwrap()["items"]
        .as_array()
        .unwrap()
        .contains(number)
    {}
}

#[test]
fn started_with_no_network_on_a_synced_database_tsugiki_shows_what_it_holds_and_marks_seen() {
    let (folders, root) = temp_folders("wired-offline-start");
    let wired = Wired::start(Ok(folders.clone()), recordings());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let synced = workbench_after_sync(&wired);
    let number = issue_over(&synced, 154_019)["number"].clone();
    let issue = json!({ "repository": CPYTHON, "number": number });
    let pr = json!({ "repository": CPYTHON, "number": 154_019 });
    // Reading a PR's Conversation keeps it, as a read ahead does.
    drop(wired.ask("open_pull_request", pr.clone()).unwrap());
    while wired.ask("pull_request", pr.clone()).unwrap()["content"].is_null() {
        let _changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
    }
    drop(wired.stop());

    let offline = Wired::start(Ok(folders), no_network());
    let workbench = offline.ask("workbench", the_whole_workbench()).unwrap();
    let held = offline.ask("issue", issue.clone()).unwrap();
    drop(offline.ask("open_issue", issue.clone()).unwrap());
    let failed = offline.failed_reads.recv_timeout(PLENTY).unwrap();
    let conversation = offline.ask("pull_request", pr).unwrap();
    drop(offline.ask("mark_unseen", issue.clone()).unwrap());
    wait_for_change_to(&offline.workbenches, &number);
    let flagged = offline.ask("issue", issue.clone()).unwrap();
    let mark = json!({ "repository": CPYTHON, "mark": flagged["seen"]["mark"] });
    drop(offline.ask("mark_seen", mark).unwrap());
    wait_for_change_to(&offline.workbenches, &number);
    let seen = offline.ask("issue", issue).unwrap();
    drop(offline.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_is_the_recorded_work(&workbench);
    let tree = held["pull_requests"].as_array().unwrap();
    assert!(
        tree.iter().any(|entry| entry["number"] == 154_019),
        "{held}"
    );
    assert_eq!(
        failed,
        json!({ "repository": CPYTHON, "number": number, "why": "waits_on_github" })
    );
    assert!(
        conversation["content"]["body_html"].is_string(),
        "{conversation}"
    );
    assert_eq!(flagged["seen"]["state"], "marked_unseen");
    assert_eq!(
        seen["seen"]["state"], "seen",
        "Mark Seen recorded Last seen"
    );
}

#[test]
fn opening_a_pr_shows_its_row_at_once_and_its_conversation_once_github_answers() {
    let (folders, root) = temp_folders("wired-open-pr");
    let wired = Wired::start(Ok(folders), recordings());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let workbench = workbench_after_sync(&wired);
    let number = workbench["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["pull_requests"][0]["number"].as_u64())
        .unwrap();
    let pr = json!({ "repository": CPYTHON, "number": number });

    let before = wired.ask("pull_request", pr.clone()).unwrap();
    drop(wired.ask("open_pull_request", pr.clone()).unwrap());
    let mut after = Value::Null;
    while after["content"].is_null() {
        let _changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
        after = wired.ask("pull_request", pr.clone()).unwrap();
    }
    drop(wired.ask("fetch_pull_request", pr).unwrap());
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(before["number"], number);
    assert_eq!(before["content"], Value::Null);
    let description = after["content"]["body_html"].as_str().unwrap();
    assert!(description.contains("gh-152315"), "{description}");
    let first = &after["content"]["reviews"][0];
    assert_eq!(
        (&first["state"], first["threads"].as_array().map(Vec::len)),
        (&json!("COMMENTED"), Some(5)),
        "the recorded PR's oldest Review, with the threads it started"
    );
}

#[test]
fn a_name_that_isnt_a_repository_opens_nothing() {
    let (folders, root) = temp_folders("wired-open-bad-name");
    let wired = Wired::start(Ok(folders), no_network());
    let bad = json!({ "repository": "not a name", "number": 7 });

    let open = wired.ask("open_issue", bad.clone()).unwrap_err();
    let fetch = wired.ask("fetch_issue", bad.clone()).unwrap_err();
    let read = wired.ask("issue", bad.clone()).unwrap_err();
    let pr_errors: Vec<Value> = ["open_pull_request", "fetch_pull_request", "pull_request"]
        .into_iter()
        .map(|command| wired.ask(command, bad.clone()).unwrap_err()["kind"].clone())
        .collect();
    let page = crate::open_github_page("not a name", GitHubItem::Issue { number: 7 }, no_browser);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(open["kind"], "not_found");
    assert_eq!(fetch["kind"], "not_found");
    assert_eq!(read["kind"], "not_found");
    assert_eq!(pr_errors, ["not_found", "not_found", "not_found"]);
    assert!(page.is_err());
}

#[test]
fn opening_an_item_on_github_shows_its_page_and_leaves_last_seen_alone() {
    let (folders, root) = temp_folders("wired-open-on-github");
    let wired = Wired::start(Ok(folders), no_network());
    let shown = std::cell::RefCell::new(Vec::new());
    for item in [
        GitHubItem::Issue { number: 7 },
        GitHubItem::PullRequest { number: 8 },
    ] {
        crate::open_github_page(CPYTHON, item, |address| {
            shown.borrow_mut().push(address.as_str().to_owned());
            Ok(())
        })
        .unwrap();
    }
    let opened = crate::open_github_page(CPYTHON, GitHubItem::Issue { number: 9 }, no_browser);
    let wired = wired.stop();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(
        shown.into_inner(),
        [
            "https://github.com/python/cpython/issues/7",
            "https://github.com/python/cpython/pull/8",
        ]
    );
    assert!(opened.is_err(), "a browser that doesn't open is a problem");
    assert!(wired.workbenches.try_recv().is_err());
}

/// Each item the answer lists, in the order the window shows them.
fn listed(workbench: &Value) -> Vec<&Value> {
    let items = workbench["items"].as_array().unwrap().iter();
    items
        .chain(workbench["quiet"].as_array().unwrap())
        .collect()
}

/// The recordings hold 21 PRs of real CPython work, and 3 of them are the recording user's own. They name
/// 21 Issues: #154019 fixes two by itself, and #156423 and #156419 fix one between them. The Filters'
/// search reaches 2 more that no PR names.
fn assert_is_the_recorded_work(workbench: &Value) {
    let items = listed(workbench);
    assert_eq!(items.len(), 23);
    assert!(
        items.iter().all(|item| item["kind"] == "issue"),
        "{items:?}"
    );
    let entries: Vec<&Value> = items
        .iter()
        .flat_map(|item| item["pull_requests"].as_array().unwrap())
        .collect();
    let rows: Vec<&Value> = entries
        .iter()
        .flat_map(|entry| {
            let beneath = entry["backports"].as_array().into_iter().flatten();
            std::iter::once(*entry).chain(beneath)
        })
        .collect();
    let under_154019 = rows.iter().filter(|row| row["number"] == 154_019);
    assert_eq!(under_154019.count(), 2);
    assert_the_recorded_tree_has_its_backports(&entries);
    let with_reason = |reason: &str| {
        rows.iter()
            .filter(|row| row["reasons"].as_array().unwrap().contains(&json!(reason)))
            .count()
    };
    assert_eq!(with_reason("own_pr"), 3);
    assert!(with_reason("review_request") > 0);
}

/// One Issue's tree comes back from real GitHub: a merged Origin PR with the bot's Backports and one a
/// person opened beneath it.
fn assert_the_recorded_tree_has_its_backports(entries: &[&Value]) {
    let origin = entries
        .iter()
        .find(|entry| entry["number"] == 151_968)
        .unwrap();
    assert_eq!(origin["kind"], "pr");
    assert_eq!(origin["state"], "MERGED");
    let kind_of = |number: u32| {
        let backports = origin["backports"].as_array().unwrap();
        let found = backports.iter().find(|row| row["number"] == number);
        found.unwrap()["backport"].clone()
    };
    assert_eq!(kind_of(152_837), "bot");
    assert_eq!(kind_of(152_840), "manual");
}

/// Waits for the session to save what each starter Filter found, the PRs, their Issues, one Issue's tree
/// and the Origin PR that tree was missing, and returns the Workbench after.
fn workbench_after_sync(wired: &Wired) -> Value {
    for _ in ["Filter", "Filter", "PRs", "Issues", "tree", "Origin PR"] {
        let changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
        assert_eq!(changed["repository"], CPYTHON);
    }
    wired.ask("workbench", the_whole_workbench()).unwrap()
}

#[test]
fn the_app_syncs_the_repository_it_reopens() {
    let (folders, root) = temp_folders("wired-reopen");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    drop(wired.stop());

    let reopened = Wired::start(Ok(folders), recordings());
    let workbench = workbench_after_sync(&reopened);
    drop(reopened.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_is_the_recorded_work(&workbench);
}

#[test]
fn a_filter_edited_in_the_file_shows_in_the_sidebar_and_the_workbench_after_a_restart() {
    const EDITED: &str = "is:open author:@me label:type-bug";
    let (folders, root) = temp_folders("wired-filter-edit");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    drop(wired.stop());
    let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
    let edited = settings
        .replace(r#"name = "Mine""#, r#"name = "My bugs""#)
        .replace(
            r#"query = "is:open author:@me""#,
            &format!(r#"query = "{EDITED}""#),
        );
    std::fs::write(folders.settings_file(), edited).unwrap();

    let reopened = Wired::start(Ok(folders), recordings());
    let whole = workbench_after_sync(&reopened);
    let selected = reopened
        .ask(
            "workbench",
            json!({ "repository": CPYTHON, "view": { "kind": "filter", "query": EDITED } }),
        )
        .unwrap();
    drop(reopened.stop());
    std::fs::remove_dir_all(root).unwrap();

    let line = &whole["filters"][0];
    assert_eq!(line["name"], "My bugs");
    assert_eq!(line["query"], EDITED);
    assert_eq!(selected["filters"], whole["filters"]);
    // The search found 7 items. #157178 shares a row with the Issue it fixes, which the search found too,
    // and #156423 and #156419 share one with the Issue they fix.
    assert_eq!(listed(&selected).len(), 5);
}

#[test]
fn a_maintained_repository_starts_with_its_own_filters() {
    let (folders, root) = temp_folders("wired-maintained");
    let wired = Wired::start(Ok(folders.clone()), no_network());
    let adding = json!({
        "repository": { "name": CPYTHON, "access": "maintained", "source": { "kind": "download" } },
    });

    let added = wired.ask("add_repository", adding).unwrap();
    drop(wired.stop());
    let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(added["repository"]["filters"][0]["name"], "Open Issues");
    assert!(settings.contains(r#"name = "Open PRs""#), "{settings}");
}

/// Answers a refresh in which Issue `number` got a new title, and says what it was asked.
struct Refreshing {
    number: u64,
    asked: Sender<String>,
}

impl Transport for Refreshing {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        drop(self.asked.send(request.operation.to_owned()));
        let rate = json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" });
        let page = |nodes: Value| json!({ "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": nodes });
        let data = match request.operation {
            "Viewer" => json!({ "viewer": { "login": "octocat" } }),
            "RefreshRepository" => json!({
                "repository": {
                    "openIssues": { "totalCount": 9000 },
                    "openPrs": { "totalCount": 0 },
                    "issues": page(json!([{
                        "number": self.number,
                        "title": "A new title",
                        "updatedAt": "2999-01-01T00:00:00Z",
                        "state": "OPEN",
                        "stateReason": null,
                        "assignees": { "nodes": [] },
                        "labels": { "nodes": [] },
                        "timelineItems": { "nodes": [] },
                        "author": null,
                    }])),
                    "pullRequests": page(json!([])),
                },
                "rateLimit": rate,
            }),
            "FetchCrossReferences" => {
                json!({ "repository": { "issue": null }, "rateLimit": rate })
            }
            _ => json!({ "repository": {}, "rateLimit": rate }),
        };
        Ok(Reply {
            status: 200,
            reset: None,
            date: None,
            body: json!({ "data": data }).to_string(),
        })
    }
}

fn wait_for(asked: &Receiver<String>, operation: &str) {
    while asked.recv_timeout(PLENTY).unwrap() != operation {}
}

/// The first thing the first item of a Workbench says about itself.
fn first_issue(workbench: &Value) -> &Value {
    &workbench["items"][0]
}

#[test]
fn a_later_start_refreshes_shows_the_change_and_a_click_asks_again() {
    let (folders, root) = temp_folders("wired-refresh");
    let wired = Wired::start(Ok(folders.clone()), recordings());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let workbench = workbench_after_sync(&wired);
    let never = wired.last_syncs.recv_timeout(PLENTY).unwrap();
    let first = wired.last_syncs.recv_timeout(PLENTY).unwrap();
    drop(wired.stop());
    let number = first_issue(&workbench)["number"].as_u64().unwrap();

    let (asked, questions) = channel();
    let access = GitHubAccess {
        transport: Box::new(Refreshing { number, asked }),
        run_gh: signed_in_gh,
    };
    let reopened = Wired::start(Ok(folders), access);
    let shown = reopened.last_syncs.recv_timeout(PLENTY).unwrap();
    let changed = reopened.workbenches.recv_timeout(PLENTY).unwrap();
    let after = reopened.ask("workbench", the_whole_workbench()).unwrap();
    let refreshed = reopened.last_syncs.recv_timeout(PLENTY).unwrap();
    let asked_for_it = reopened.ask("last_sync", json!({})).unwrap();
    wait_for(&questions, "RefreshRepository");
    let clicked = reopened.ask("refresh", json!({}));
    wait_for(&questions, "RefreshRepository");
    drop(reopened.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(never, json!({ "repository": CPYTHON, "at": null }));
    assert_eq!(first["repository"], CPYTHON);
    assert_eq!(shown, first, "a later start says when the last sync was");
    assert_eq!(changed, json!({ "repository": CPYTHON, "items": [number] }));
    assert_eq!(first_issue(&after)["title"], "A new title");
    assert!(refreshed["at"].is_string(), "{refreshed}");
    assert_eq!(asked_for_it, refreshed);
    assert_eq!(clicked.unwrap(), Value::Null);
}

#[test]
fn an_add_syncs_what_waits_on_the_user_and_the_status_bar_hears_each_part() {
    let (folders, root) = temp_folders("wired-activity");
    let wired = Wired::start(Ok(folders), recordings());
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let workbench = workbench_after_sync(&wired);

    let mut parts = Vec::new();
    let ended = loop {
        let heard = wired.activities.recv_timeout(PLENTY).unwrap();
        if heard.is_null() {
            break heard;
        }
        parts.push(heard["open"]["part"].as_str().unwrap().to_owned());
    };
    parts.dedup();
    let asked = wired.ask("sync_activity", json!({})).unwrap();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_is_the_recorded_work(&workbench);
    assert_eq!(parts, ["refresh", "your_work", "trees", "window"]);
    assert_eq!(ended, Value::Null);
    assert_eq!(asked, Value::Null);
}

#[test]
fn a_window_that_loads_before_the_core_knows_hears_no_last_sync() {
    let wired = Wired::start(Err(no_folders()), no_network());

    let asked = wired.ask("last_sync", json!({}));
    let clicked = wired.ask("refresh", json!({}));
    drop(wired.stop());

    assert_eq!(asked.unwrap(), Value::Null);
    assert_eq!(clicked.unwrap(), Value::Null);
}

#[test]
fn with_no_repository_the_app_has_nothing_to_sync() {
    let (folders, root) = temp_folders("wired-nothing");
    let wired = Wired::start(Ok(folders), recordings());

    drop(wired.statuses.recv_timeout(PLENTY).unwrap());
    let wired = wired.stop();
    std::fs::remove_dir_all(root).unwrap();

    assert!(wired.workbenches.try_recv().is_err());
}

#[test]
fn a_change_the_window_cant_hear_leaves_only_a_trace() {
    log_if_unsent(Err(tauri::Error::WindowNotFound));
}

/// Bare repositories with one commit each under `root/remotes`, laid out as GitHub's are, for mirrors to
/// clone.
fn remotes(root: &Path, names: &[&str]) -> Remotes {
    let remotes = root.join("remotes");
    for name in names {
        tsugiki_test_git::remote(&remotes, name);
    }
    Remotes::under(tsugiki_test_git::url(&remotes))
}

/// The Repositories each clone announced, in turn, as the webview heard them.
fn clones(wired: &Wired, count: usize) -> Vec<Vec<Value>> {
    (0..count)
        .map(|_| {
            let mut clone = Vec::new();
            loop {
                let heard = wired.clonings.recv_timeout(PLENTY).unwrap();
                if heard.is_null() {
                    break clone;
                }
                clone.push(heard);
            }
        })
        .collect()
}

#[test]
fn adding_a_repository_shows_its_clone_until_it_ends() {
    let (folders, root) = temp_folders("wiring-clone");
    let remotes = remotes(&root, &[CPYTHON]);
    let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);

    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let clone = clones(&wired, 1).remove(0);
    let asked = wired.ask("cloning", json!({})).unwrap();
    drop(wired.stop());
    let mirror = folders.mirror(&tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap());
    let cloned = mirror.join("HEAD").is_file();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(
        clone.first(),
        Some(&json!({ "repository": CPYTHON, "progress": { "stage": "starting" } }))
    );
    assert!(clone.contains(
        &json!({ "repository": CPYTHON, "progress": { "stage": "receiving", "percent": 100 } })
    ));
    assert_eq!(asked, Value::Null);
    assert!(cloned);
}

#[test]
fn the_window_hears_each_mirrors_state_as_it_changes_and_can_ask_for_it() {
    let (folders, root) = temp_folders("wiring-mirror-states");
    let remotes = remotes(&root, &[CPYTHON]);
    let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let ready = json!([{ "repository": CPYTHON, "state": { "state": "ready" } }]);
    let mut heard = Vec::new();
    while heard.last() != Some(&ready) {
        heard.push(wired.states.recv_timeout(PLENTY).unwrap());
    }
    let asked = wired.ask("mirror_states", json!({})).unwrap();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let starting = json!([{
        "repository": CPYTHON,
        "state": { "state": "cloning", "progress": { "stage": "starting" } },
    }]);
    assert!(heard.contains(&starting), "{heard:?}");
    assert_eq!(asked, ready);
}

#[test]
fn a_disk_short_of_room_for_a_clone_says_so_and_check_again_looks_once_more() {
    let (folders, root) = temp_folders("wiring-disk-short");
    let (measuring, measures) = channel();
    let remotes = Remotes::under("no-remotes/".to_owned()).on_disk(
        Box::new(|_| Ok(5 << 30)),
        Box::new(move |_| {
            measuring.send(()).unwrap_or_default();
            Ok(1 << 30)
        }),
    );
    let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let short = wired.problems.recv_timeout(PLENTY).unwrap();
    let asked = wired.ask("mirror_problem", json!({})).unwrap();
    let _ = wired.ask("check_mirrors_again", json!({})).unwrap();
    let measured = measures
        .recv_timeout(PLENTY)
        .and(measures.recv_timeout(PLENTY));
    // The check before Check again's measure would have said so by now.
    let heard = wired.problems.try_recv();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(short["kind"], "disk_short");
    assert_eq!(short["repository"], CPYTHON);
    assert_eq!(short["needed_mb"], 6 * 1024);
    assert_eq!(short["free_mb"], 1024);
    assert_eq!(asked, short);
    assert!(measured.is_ok(), "Check again measures the disk once more");
    assert!(
        heard.is_err(),
        "a disk still short is nothing new: {heard:?}"
    );
}

#[test]
fn a_repository_from_before_asks_where_its_mirror_gets_objects_and_borrows_on_the_answer() {
    let (folders, root) = temp_folders("wiring-borrow");
    let mut files = SettingsFiles::load(folders.clone());
    let cpython = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
    let _ = files
        .add_repository(
            &RepositorySettings::added(cpython.clone(), RepositoryAccess::Other),
            &MirrorSource::Download,
            time::OffsetDateTime::now_utc(),
        )
        .unwrap();
    // As 0.1 left it: the Repository, and no answer.
    std::fs::write(
        folders.machine_file(),
        "last_repository = \"python/cpython\"\n",
    )
    .unwrap();
    let remotes = remotes(&root, &[CPYTHON]);
    let lender = root.join("user-cpython");
    let url = remotes.url(&cpython);
    let _ = git(&root, &["clone", "-q", &url, &lender.to_string_lossy()]);
    let picked = lender.clone();
    let wired = Wired::start_picking(
        Ok(folders.clone()),
        recordings(),
        remotes,
        Arc::new(move || Some(picked.clone())),
    );

    let question = wired.questions.recv_timeout(PLENTY).unwrap();
    let asked = wired.ask("mirror_question", json!({})).unwrap();
    let picked = wired.ask("pick_folder", json!({})).unwrap();
    let folder = |path: &Path| json!({ "repository": CPYTHON, "folder": path });
    let refused = wired
        .ask("check_clone", folder(&root.join("gone")))
        .unwrap();
    let lends = wired.ask("check_clone", folder(&lender)).unwrap();
    let borrow = json!({ "kind": "borrow", "folder": lender });
    let chose = wired.ask(
        "choose_mirror_source",
        json!({ "repository": CPYTHON, "source": borrow }),
    );
    let answered = wired.questions.recv_timeout(PLENTY).unwrap();
    let cloned = clones(&wired, 1);
    drop(wired.stop());
    let machine = std::fs::read_to_string(folders.machine_file()).unwrap();
    let alternates = folders
        .mirror(&cpython)
        .join("objects/info/alternates")
        .exists();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(question, json!(CPYTHON));
    assert_eq!(asked, json!(CPYTHON));
    assert_eq!(picked, json!(lender));
    assert_eq!(refused, json!({ "kind": "not_a_folder" }));
    assert_eq!(lends, Value::Null);
    assert_eq!(chose, Ok(Value::Null));
    assert_eq!(answered, Value::Null);
    assert_eq!(cloned.len(), 1);
    assert!(machine.contains("[mirrors]"), "{machine}");
    assert!(!alternates);
}

#[test]
fn an_answer_machine_toml_cant_keep_still_starts_the_clone() {
    let (folders, root) = temp_folders("wiring-answer-unsaved");
    let mut files = SettingsFiles::load(folders.clone());
    let cpython = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
    let _ = files
        .add_repository(
            &RepositorySettings::added(cpython.clone(), RepositoryAccess::Other),
            &MirrorSource::Download,
            time::OffsetDateTime::now_utc(),
        )
        .unwrap();
    std::fs::write(
        folders.machine_file(),
        "last_repository = \"python/cpython\"\n",
    )
    .unwrap();
    let wired = Wired::start_cloning_from(
        Ok(folders.clone()),
        recordings(),
        remotes(&root, &[CPYTHON]),
    );

    let _ = wired.questions.recv_timeout(PLENTY).unwrap();
    // Broken after the launch read it, as a user's edit in another window would leave it.
    std::fs::write(folders.machine_file(), "last_repository = \n").unwrap();
    let chose = wired.ask(
        "choose_mirror_source",
        json!({ "repository": CPYTHON, "source": { "kind": "download" } }),
    );
    let answered = wired.questions.recv_timeout(PLENTY).unwrap();
    let cloned = clones(&wired, 1);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert!(chose.is_err(), "{chose:?}");
    assert_eq!(answered, Value::Null);
    assert_eq!(cloned.len(), 1);
}

#[test]
fn a_picker_the_user_cancels_gives_nothing() {
    let wired = Wired::start(Err(no_folders()), no_network());

    let picked = wired.ask("pick_folder", json!({}));
    drop(wired.stop());

    assert_eq!(picked, Ok(Value::Null));
}

/// Adds each of `names` to the settings in `folders` before a launch, to download its mirror, so the last one
/// opens.
fn added_before_launch(folders: &Folders, names: &[&str]) {
    let mut files = SettingsFiles::load(folders.clone());
    for name in names {
        let name = RepositoryName::parse(name).unwrap();
        let _ = files
            .add_repository(
                &RepositorySettings::added(name, RepositoryAccess::Other),
                &MirrorSource::Download,
                time::OffsetDateTime::now_utc(),
            )
            .unwrap();
    }
}

/// The Repository each clone was of, in turn.
fn clone_order(wired: &Wired, count: usize) -> Vec<Value> {
    clones(wired, count)
        .into_iter()
        .map(|clone| clone[0]["repository"].clone())
        .collect()
}

#[test]
fn repositories_added_before_get_their_mirrors_at_launch_the_open_one_first() {
    let (folders, root) = temp_folders("wiring-clones-at-launch");
    added_before_launch(&folders, &["octo-org/first", CPYTHON]);
    let remotes = remotes(&root, &["octo-org/first", CPYTHON]);
    let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

    let order = clone_order(&wired, 2);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(order, [json!(CPYTHON), json!("octo-org/first")]);
}

#[test]
fn a_switch_puts_the_repositorys_clone_next_after_the_one_under_way() {
    let (folders, root) = temp_folders("wiring-switch-clones");
    let names = ["octo-org/first", "octo-org/second", CPYTHON];
    added_before_launch(&folders, &names);
    // The open Repository's clone waits on its size until the switch is in, so the others can't start first.
    let (release, gate) = channel::<()>();
    let cpython = RepositoryName::parse(CPYTHON).unwrap();
    let remotes = remotes(&root, &names).on_disk(
        Box::new(move |name| {
            if name.is_same(&cpython) {
                gate.recv().unwrap_or_default();
            }
            Ok(0)
        }),
        Box::new(|_| Ok(u64::MAX)),
    );
    let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

    let opened = wired.ask("open_repository", json!({ "name": "octo-org/second" }));
    drop(release);
    let order = clone_order(&wired, 3);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert!(opened.is_ok(), "{opened:?}");
    assert_eq!(
        order,
        [
            json!(CPYTHON),
            json!("octo-org/second"),
            json!("octo-org/first")
        ]
    );
}

/// Searches the diff of PR 1 in python/cpython between `head` and `base` for `query`.
fn find_in_pr_1(wired: &Wired, (head, base): (&str, &str), query: &str) -> Result<Value, Value> {
    let search =
        json!({ "repository": CPYTHON, "number": 1, "head": head, "base": base, "query": query });
    wired.ask("pull_request_find", json!({ "search": search }))
}

/// Puts a commit of PR `number`'s own on the remote for `name` among the [`remotes`] under `root`, and
/// gives the commit.
fn pull_request_on(root: &Path, name: &str, number: u32) -> String {
    tsugiki_test_git::pull_request(&root.join("remotes"), name, number)
}

#[test]
fn a_sync_fetches_the_heads_of_the_users_prs_into_the_mirror() {
    let (folders, root) = temp_folders("wiring-heads");
    let remotes = remotes(&root, &[CPYTHON]);
    // The user's own PR in the recordings.
    let commit = pull_request_on(&root, CPYTHON, 157_178);
    let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);

    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let _clone = clones(&wired, 1);
    let mirror = folders.mirror(&tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap());
    let fetched = mirror.join("refs/tsugiki/pull/157178/head");
    let deadline = Instant::now() + PLENTY;
    while !fetched.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(wired.stop());
    let head = std::fs::read_to_string(&fetched).unwrap_or_default();
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(head.trim(), commit);
}

/// PR #1 of cpython, someone else's, at `commit`, which no sync step reads.
fn someone_elses_pr_1(folders: &Folders, commit: &str) {
    let pr = PullRequestFacts {
        number: 1,
        title: "gh-1: Fix it".to_owned(),
        state: PrState::Open,
        is_draft: false,
        author: Some(Login::new("some-author")),
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: time::OffsetDateTime::UNIX_EPOCH,
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: CommitSha::parse(commit),
        off_default: None,
    };
    let name = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
    let store = tsugiki_core::store::Store::open(&folders.database()).unwrap();
    store.save_pull_requests(&name, vec![pr]).unwrap();
    store.close();
}

#[test]
fn opening_a_pr_fetches_its_head_first_and_its_files_tab_then_reads_its_diff_in_chunks() {
    let (folders, root) = temp_folders("wiring-files");
    let remotes = remotes(&root, &[CPYTHON]);
    let commit = pull_request_on(&root, CPYTHON, 1);
    someone_elses_pr_1(&folders, &commit);
    let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let _clone = clones(&wired, 1);
    let files = || {
        wired.ask(
            "pull_request_files",
            json!({ "repository": CPYTHON, "number": 1 }),
        )
    };
    let chunk_of = |head: &str, base: &str| json!({ "repository": CPYTHON, "number": 1, "head": head, "base": base, "start": 0, "end": 512 });
    let chunk = |head: &str, base: &str| {
        wired.ask(
            "pull_request_rows",
            json!({ "chunk": chunk_of(head, base) }),
        )
    };

    drop(
        wired
            .ask(
                "open_pull_request",
                json!({ "repository": CPYTHON, "number": 1 }),
            )
            .unwrap(),
    );
    let heard = wired.heads.recv_timeout(PLENTY).unwrap();
    let ready = files().unwrap();
    let base = ready["base"].as_str().unwrap_or_default().to_owned();
    let rows = chunk(&commit, &base).unwrap();
    let spans = wired.ask(
        "pull_request_spans",
        json!({ "chunk": chunk_of(&commit, &base), "collapsed": [] }),
    );
    let lines_of = |start: u32, end: u32| {
        wired.ask(
                "pull_request_lines",
                json!({ "lines": { "repository": CPYTHON, "number": 1, "head": commit, "base": base, "file": 0, "start": start, "end": end } }),
            )
    };
    let lines = lines_of(1, 21);
    let found = find_in_pr_1(&wired, (&commit, &base), "pr");
    let from_0 = lines_of(0, 21).unwrap_err();
    let backwards = lines_of(21, 1).unwrap_err();
    let no_commit = chunk("main", &base).unwrap_err();
    let no_base = chunk(&commit, "main").unwrap_err();
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(heard, json!({ "repository": CPYTHON, "items": [1] }));
    let news = json!({
        "path": "NEWS",
        "old_path": null,
        "change": { "kind": "lines", "added": 1, "removed": 0 },
        "row": 0,
        "columns": 4,
        "generated": false,
        "gaps": [],
        "rest": null,
        "pairs": [{ "row": 2, "removed": 0, "added": 1 }]
    });
    assert_eq!(
        ready,
        json!({ "state": "ready", "head": commit, "base": base, "files": [news], "rows": 3, "news": [], "threads": { "kind": "shown", "threads": [] } })
    );
    assert!(CommitSha::parse(&base).is_some_and(|base| base.as_str() != commit));
    assert_eq!(
        rows,
        json!([
            {
                "kind": "file",
                "path": "NEWS",
                "old_path": null,
                "change": { "kind": "lines", "added": 1, "removed": 0 }
            },
            { "kind": "hunk", "text": "@@ -0,0 +1 @@" },
            { "kind": "added", "new": 1, "text": "PR 1" }
        ])
    );
    assert_eq!(no_commit["message"], "\"main\" isn't a commit.");
    assert_eq!(no_base["message"], "\"main\" isn't a commit.");
    // NEWS is in no language Tsugiki highlights.
    assert_eq!(spans.unwrap(), json!([[], [], []]));
    // The file has one line, read from the mirror.
    // A search finds the line too, whatever the case.
    let found_line = json!({ "matches": [{ "row": 2, "start": 0, "end": 2 }], "more": false });
    assert_eq!(
        (lines.unwrap(), found.unwrap()),
        (json!([{ "text": "PR 1", "spans": [] }]), found_line)
    );
    assert_eq!(from_0["message"], "Lines 0 to 21 aren't lines of a file.");
    assert_eq!(
        backwards["message"],
        "Lines 21 to 1 aren't lines of a file."
    );
}

#[test]
fn a_failed_fetch_of_a_prs_head_reaches_the_window_with_why() {
    let (folders, root) = temp_folders("wiring-failed-head");
    let remotes = remotes(&root, &[CPYTHON]);
    let commit = pull_request_on(&root, CPYTHON, 1);
    someone_elses_pr_1(&folders, &commit);
    let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);
    let _ = wired.ask("add_repository", add_cpython()).unwrap();
    let _clone = clones(&wired, 1);
    // GitHub's copy goes after the clone, so the fetch of the head fails.
    let bare = root.join("remotes").join(format!("{CPYTHON}.git"));
    std::fs::remove_dir_all(bare).unwrap();

    let fetching = wired.ask(
        "pull_request_files",
        json!({ "repository": CPYTHON, "number": 1 }),
    );
    // The sync's heads may fail in a fetch of their own before PR #1's.
    let names_1 = |heard: &Value| {
        heard["items"]
            .as_array()
            .is_some_and(|items| items.contains(&json!(1)))
    };
    let heard = std::iter::from_fn(|| wired.failed_heads.recv_timeout(PLENTY).ok()).find(names_1);
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(fetching.unwrap(), json!({ "state": "fetching" }));
    let heard = heard.unwrap();
    assert_eq!(heard["repository"], CPYTHON);
    assert_eq!(heard["why"]["kind"], "git");
    let words = heard["why"]["words"].as_str();
    assert!(words.is_some_and(|words| !words.is_empty()));
}

/// A mirror of cpython in `folders` whose `Lib/spam.py` has 30 lines on main, and PR #1's commit, which
/// changes line 15, on the branch `pr`. Gives the PR's commit.
fn mirror_with_pr_1(folders: &Folders, root: &Path) -> String {
    let work = root.join("work");
    std::fs::create_dir_all(work.join("Lib")).unwrap();
    let lines = |line_15: &str| {
        (1..=30)
            .map(|line| match line {
                15 => format!("{line_15}\n"),
                _ => format!("x{line} = {line}\n"),
            })
            .collect::<String>()
    };
    let _ = git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("Lib/spam.py"), lines("x15 = 15")).unwrap();
    let _ = git(&work, &["add", "."]);
    let _ = git(&work, &["commit", "-qm", "One"]);
    let _ = git(&work, &["switch", "-qc", "pr"]);
    std::fs::write(work.join("Lib/spam.py"), lines("x15 = 'fifteen'")).unwrap();
    let _ = git(&work, &["commit", "-qam", "PR 1"]);
    let commit = git(&work, &["rev-parse", "HEAD"]).trim().to_owned();
    let mirror = folders.mirror(&RepositoryName::parse(CPYTHON).unwrap());
    std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    let (work, mirror) = (work.to_string_lossy(), mirror.to_string_lossy());
    let _ = git(root, &["clone", "-q", "--bare", &work, &mirror]);
    commit
}

#[test]
fn offline_a_lookup_asks_ty_in_the_lookup_checkout_at_a_commit_the_mirror_holds() {
    let (folders, root) = temp_folders("wiring-offline-definitions");
    let head = mirror_with_pr_1(&folders, &root);
    someone_elses_pr_1(&folders, &head);
    // The ty that `just cargo-test` and `just rust-coverage` fetch.
    let ty = Path::new(&std::env::var_os("TSUGIKI_TY").unwrap_or_default()).to_owned();
    assert!(ty.is_file(), "TSUGIKI_TY={ty:?}: see just fetch-ty");
    let machine = format!("[language_servers.ty]\npath = '{}'\n", ty.display());
    std::fs::write(folders.machine_file(), machine).unwrap();
    let wired = Wired::start(Ok(folders), no_network());

    let _held = wired.held.recv_timeout(PLENTY).unwrap();
    let pr = json!({ "repository": CPYTHON, "number": 1 });
    let files = wired.ask("pull_request_files", pr).unwrap();
    let base = files["base"].as_str().unwrap_or_default().to_owned();
    let look = |side: &str, path: &str| {
        let lookup = json!({ "repository": CPYTHON, "base": base, "head": head, "path": path, "side": side, "line": 15, "column": 0 });
        // The window hears what the lookup waits for on a channel of its own.
        wired.ask(
            "definitions",
            json!({ "lookup": lookup, "waiting": "__CHANNEL__:7" }),
        )
    };
    let at_head = look("head", "Lib/spam.py");
    let at_base = look("base", "Lib/spam.py");
    let other = look("head", "Lib/spam.c");
    // The peek shows the hit's code from the mirror, at the hit's commit.
    let code = |commit: &str| {
        let file = json!({ "kind": "repository", "path": "Lib/spam.py", "commit": commit });
        let code = json!({ "repository": CPYTHON, "file": file, "start": 14, "end": 17 });
        wired.ask("definition_code", json!({ "code": code }))
    };
    let code_at_head = code(&head);
    let code_at_base = code(&base);
    // A file outside the Repository can be read only once a server named it in a hit.
    let outside = json!({ "kind": "outside", "path": "C:/elsewhere/secret.txt" });
    let unnamed = json!({ "repository": CPYTHON, "file": outside, "start": 1, "end": 2 });
    let unnamed = wired.ask("definition_code", json!({ "code": unnamed }));
    // With no lookup running, a cancel does nothing.
    let cancelled = wired.ask("cancel_definitions", json!({}));
    let refused = wired.ask(
        "definitions",
        json!({ "lookup": { "repository": CPYTHON, "base": "main", "head": head, "path": "Lib/spam.py", "side": "head", "line": 1, "column": 0 }, "waiting": "__CHANNEL__:8" }),
    );
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    let hit = |side: &str, commit: &str, text: &str| {
        let place =
            json!({ "kind": "repository", "path": "Lib/spam.py", "side": side, "commit": commit });
        json!({ "state": "found", "hits": [{ "place": place, "line": 15, "column": 0, "text": text }] })
    };
    assert_eq!(at_head, Ok(hit("head", &head, "x15 = 'fifteen'")));
    assert_eq!(at_base, Ok(hit("base", &base, "x15 = 15")));
    assert_eq!(other, Ok(json!({ "state": "no_server" })));
    let lines = |line_15: &str| json!({ "count": 30, "lines": ["x14 = 14", line_15, "x16 = 16"] });
    assert_eq!(code_at_head, Ok(lines("x15 = 'fifteen'")));
    assert_eq!(code_at_base, Ok(lines("x15 = 15")));
    let unnamed = unnamed.unwrap_err()["message"].clone();
    assert_eq!(unnamed, "No language server named C:/elsewhere/secret.txt.");
    assert_eq!(cancelled, Ok(Value::Null));
    assert_eq!(refused.unwrap_err()["message"], "\"main\" isn't a commit.");
}

/// What PR #1 says as GitHub had it at `head`: a Review with a thread on line 15, which the diff shows, one
/// on line 25, which it doesn't, and an Outdated one.
fn conversation_of_pr_1(head: &str) -> PullRequestContent {
    let thread = |id: &str, line: Option<u32>| ThreadLine {
        id: ThreadId::new(id),
        path: "Lib/spam.py".to_owned(),
        subject: Subject::Line {
            side: Side::Right,
            line,
            original: Some(line.unwrap_or(7)),
        },
        opening: Opening::Words {
            words: "Why?".to_owned(),
        },
        replies: 0,
        resolution: Resolution::Open,
        anchor: if line.is_some() {
            Anchor::Current
        } else {
            Anchor::Outdated
        },
        comments: vec![ThreadComment {
            id: None,
            comment: Comment {
                author: Some(Login::new("some-reviewer")),
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Why?</p>".to_owned(),
                hidden: None,
            },
            submission: Submission::Submitted,
        }],
        hunk: DiffHunk::parse("@@ -6,2 +6,2 @@\n x6 = 6\n-x7 = 7\n+x7 = 'seven'"),
    };
    PullRequestContent {
        conversation: IssueContent {
            body_html: String::new(),
            comments: vec![],
            earlier_comments: 0,
            opened_at: Some(time::OffsetDateTime::UNIX_EPOCH),
        },
        reviews: vec![Review {
            id: None,
            author: Some(Login::new("some-reviewer")),
            state: ReviewState::Commented,
            commit: CommitSha::parse(head),
            submitted_at: time::OffsetDateTime::UNIX_EPOCH,
            body_html: String::new(),
            hidden: None,
            threads: vec![
                thread("in-diff", Some(15)),
                thread("outside", Some(25)),
                thread("outdated", None),
            ],
        }],
        earlier_reviews: 0,
        head: CommitSha::parse(head),
        pending: vec![],
    }
}

#[test]
fn offline_a_pr_whose_head_the_mirror_holds_shows_its_diff_highlighting_and_threads() {
    let (folders, root) = temp_folders("wiring-offline-files");
    let commit = mirror_with_pr_1(&folders, &root);
    someone_elses_pr_1(&folders, &commit);
    let store = tsugiki_core::store::Store::open(&folders.database()).unwrap();
    let name = RepositoryName::parse(CPYTHON).unwrap();
    let content = conversation_of_pr_1(&commit);
    store.save_pull_request_content(&name, 1, content).unwrap();
    store.close();
    let wired = Wired::start(Ok(folders), no_network());

    let held = wired.held.recv_timeout(PLENTY).unwrap();
    let asked_held = wired.ask("mirrors_held", json!({})).unwrap();
    let pr = json!({ "repository": CPYTHON, "number": 1 });
    let files = wired.ask("pull_request_files", pr).unwrap();
    let base = files["base"].as_str().unwrap_or_default().to_owned();
    let chunk = json!({ "repository": CPYTHON, "number": 1, "head": commit, "base": base, "start": 0, "end": 512 });
    let rows = wired.ask("pull_request_rows", json!({ "chunk": chunk }));
    let spans = wired.ask(
        "pull_request_spans",
        json!({ "chunk": chunk, "collapsed": [] }),
    );
    let gap = json!({ "repository": CPYTHON, "number": 1, "head": commit, "base": base, "file": 0, "start": 1, "end": 3 });
    let lines = wired.ask("pull_request_lines", json!({ "lines": gap }));
    drop(wired.stop());
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!((held, asked_held), (json!(true), json!(true)));
    assert_eq!(files["state"], "ready");
    assert_eq!(files["head"], json!(commit));
    let rows = rows.unwrap();
    let added = json!({ "kind": "added", "new": 15, "text": "x15 = 'fifteen'" });
    assert!(rows.as_array().unwrap().contains(&added), "{rows}");
    let spans = spans.unwrap();
    let spans = spans.as_array().unwrap();
    assert_eq!(spans.len(), rows.as_array().unwrap().len());
    assert!(spans.iter().any(|row| row != &json!([])), "{spans:?}");
    let threads = &files["threads"]["threads"];
    let placed: Vec<(&Value, &Value)> = (threads.as_array().unwrap().iter())
        .map(|thread| (&thread["id"], &thread["at"]["kind"]))
        .collect();
    assert_eq!(
        placed,
        [
            (&json!("outdated"), &json!("outdated")),
            (&json!("in-diff"), &json!("row")),
            (&json!("outside"), &json!("outside")),
        ]
    );
    let lines = lines.unwrap();
    assert_eq!(lines[0]["text"], "x1 = 1");
    assert_eq!(lines[1]["text"], "x2 = 2");
    assert_ne!(lines[0]["spans"], json!([]));
}
