//! Opens Tsugiki's real window, and calls `wiring.rs` for the rest. Measuring this takes that window, so the
//! coverage gate leaves it out and the smoke tests cover it (ADR 0009).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, RunEvent, WebviewWindowBuilder};
use tsugiki_core::build::Build;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::launch;
use tsugiki_core::logfile;
use tsugiki_core::mirror::Remotes;
use tsugiki_token::{Https, run_gh};

use crate::Started;
use crate::navigation;
use crate::specta_builder;
use crate::version;
use crate::wiring::{Outside, connect_core};

/// How often Tsugiki asks GitHub whether it is still there, at about one point of the hourly 5,000 each.
const REFRESH: Duration = Duration::from_secs(60);

/// How long the process waits for the session to close the database after its window has gone. A request to
/// GitHub, a call to `gh` or a migration can use all of it. Flushing the log can then add up to another
/// second.
const QUIT_PATIENCE: Duration = Duration::from_secs(1);

/// Starts Tsugiki's window.
#[expect(
    clippy::expect_used,
    reason = "without a window the user has nowhere to read an error, so a failure ends the program"
)]
pub fn run() {
    let started = Started::now();
    let folders = Folders::current();
    if let Ok(found) = &folders {
        logfile::start(found.log());
    }
    log_panics();
    let builder = specta_builder();
    let mut context = tauri::generate_context!();
    // The single-instance lock is named after this, so a development build runs beside an installed Tsugiki.
    Build::CURRENT
        .identifier()
        .clone_into(&mut context.config_mut().identifier);
    let webview_data = folders.as_ref().ok().map(Folders::webview);
    // A second `tsg` gives its command line to the first and exits inside `build`, before it has a window, a
    // session or anything to log. It has to be the first plugin for that.
    let tauri_builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            launch::open_command_line(Build::CURRENT, &argv);
            bring_forward(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            open_window(app.handle(), webview_data)?;
            turn_off_browser_keys(app.handle());
            Ok(())
        })
        .invoke_handler(builder.invoke_handler());
    #[cfg(all(windows, debug_assertions))]
    let tauri_builder = tauri_builder.plugin(tauri_plugin_deep_link::init());
    let app = tauri_builder
        .build(context)
        .expect("Tsugiki failed to start");
    assert!(app.manage(started), "only this manages the start");
    logfile::say_started(&version());
    #[cfg(all(windows, debug_assertions))]
    register_development_scheme(app.handle());
    launch::open_command_line(Build::CURRENT, &launch::command_line());
    builder.mount_events(&app);
    let outside = Outside {
        github: GitHubAccess {
            transport: Box::new(Https::new()),
            run_gh,
        },
        releases: {
            let https = Https::new();
            Box::new(move |path| https.get_public(path))
        },
        remotes: Remotes::github(),
        pick_folder: {
            let app = app.handle().clone();
            Arc::new(move || pick_folder(&app))
        },
    };
    let core = connect_core(app.handle(), folders, outside, REFRESH);
    app.run(move |_app, event| {
        if let RunEvent::Exit = event {
            core.stop(QUIT_PATIENCE);
            log::logger().flush();
        }
        // On macOS the OS hands a link to the running app rather than starting a second `tsg`.
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = event {
            for url in urls {
                launch::open(Build::CURRENT, url.as_str());
            }
            bring_forward(_app);
        }
    });
}

/// Opens the window `tauri.conf.json` describes, keeping the webview's data with Tsugiki's so a portable copy
/// stays in its folder (ADR 0014). The file can't say this itself, because it takes only a path inside
/// Tauri's folder. Without `webview_data` the webview uses Tauri's folder, and the session shows why there
/// are none.
fn open_window(app: &AppHandle, webview_data: Option<PathBuf>) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.first() else {
        return Err(tauri::Error::WindowNotFound);
    };
    let dev_url = tauri::is_dev()
        .then(|| app.config().build.dev_url.clone())
        .flatten();
    let window = WebviewWindowBuilder::from_config(app, config)?
        .on_navigation(move |url| {
            let allowed = navigation::stays_in_tsugiki(url, dev_url.as_ref());
            if !allowed {
                log::warn!("Tsugiki kept its window from going to {url}");
            }
            allowed
        })
        .on_new_window(|url, _| {
            log::warn!("Tsugiki kept a window from opening on {url}");
            NewWindowResponse::Deny
        })
        // The page names the open Repository in its title, and the taskbar and Alt+Tab show the window's.
        .on_document_title_changed(|window, title| {
            if let Err(cause) = window.set_title(&title) {
                log::warn!("Tsugiki can't change its window's title: {cause}");
            }
        });
    let window = match webview_data {
        Some(folder) => window.data_directory(folder),
        None => window,
    };
    // Tauri keeps the window, and `get_webview_window` finds it again.
    window.build().map(drop)
}

/// Brings the window to the front of the user's desk, for a second `tsg` or a link. Before the window exists
/// there's nothing to do, because it opens in front anyway.
fn bring_forward(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let steps = [window.unminimize(), window.show(), window.set_focus()];
        for cause in steps.into_iter().filter_map(Result::err) {
            log::warn!("Tsugiki can't bring its window forward: {cause}");
        }
    }
}

/// Stops a release's webview seeing Print, Reload and the rest, so Tsugiki's keys reach Tsugiki (ADR 0013). A
/// development build keeps them, for DevTools. The window exists by now, because `open_window` ran first.
#[cfg(windows)]
fn turn_off_browser_keys(app: &AppHandle) {
    if Build::CURRENT != Build::Release {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        log::error!("Tsugiki has no window to turn the browser's shortcuts off in");
        return;
    };
    let turned_off = window.with_webview(|webview| {
        if let Err(cause) = tsugiki_webview_keys::turn_off(&webview.controller()) {
            log::error!("Tsugiki can't turn the browser's shortcuts off: {cause}");
        }
    });
    if let Err(cause) = turned_off {
        log::error!("Tsugiki can't reach the webview to turn its shortcuts off: {cause}");
    }
}

/// WKWebView has no browser shortcuts to turn off (ADR 0013).
#[cfg(not(windows))]
fn turn_off_browser_keys(_: &AppHandle) {}

/// Points `tsugiki-dev://` links at this program, in the user's own part of the registry. The installers
/// register `tsugiki` themselves, and writing it here would take the installed copy's links (ADR 0016).
#[cfg(all(windows, debug_assertions))]
fn register_development_scheme(app: &AppHandle) {
    use tauri_plugin_deep_link::DeepLinkExt;

    if let Err(cause) = app.deep_link().register(Build::CURRENT.scheme()) {
        log::warn!("Tsugiki can't register its development links: {cause}");
    }
}

/// A panic prints to a console a released app doesn't have, so its message and place also go to the log.
fn log_panics() {
    let print = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        log::error!("{panic}");
        log::logger().flush();
        print(panic);
    }));
}

/// The folder the user picks in the system's picker, over the window, or nothing if they cancel. It waits
/// for the user, so it runs off the main thread.
fn pick_folder(app: &AppHandle) -> Option<PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let mut picker = app.dialog().file();
    if let Some(window) = app.get_webview_window("main") {
        picker = picker.set_parent(&window);
    }
    picker.blocking_pick_folder()?.into_path().ok()
}
