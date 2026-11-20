use super::*;
use crate::conventions::Conventions;
use crate::filters::{Filter, OnWorkbench};
use crate::mirror_source::MirrorSource;
use crate::repository::{FoundRepository, RepositoryCounts};
use crate::testing::{TempPath, cpython, moment};
use crate::theme::{ThemeChoice, ThemeInForce, ThemeKind, ThemeName, ThemePick};

fn hidden() -> Settings {
    Settings {
        status_bar: StatusBarSettings { visible: false },
        releases: ReleaseSettings { check: true },
        appearance: AppearanceSettings {
            theme: ThemeChoice::System,
            light_theme: ThemeName::new("VS Code Light Modern"),
            dark_theme: ThemeName::new("VS Code Dark Modern"),
        },
        diff: DiffSettings {
            files: DiffFiles::All,
            peek: DiffPeek::Inline,
        },
        keys: Remaps::default(),
        repositories: Vec::new(),
    }
}

fn named(name: &str) -> ThemeName {
    ThemeName::new(name)
}

/// When a Repository was opened, for a test that doesn't look at the time.
const WHENEVER: OffsetDateTime = OffsetDateTime::UNIX_EPOCH;

fn repository(text: &str) -> RepositoryName {
    RepositoryName::parse(text).unwrap()
}

/// Adds each of `names` through the settings' own write, the first at `first`, each later one an hour after
/// the one before.
fn add_all(files: &mut SettingsFiles, names: &[&str], first: OffsetDateTime) {
    for (hours, name) in (0..).zip(names) {
        drop(
            files
                .add_repository(
                    &RepositorySettings::new(repository(name)),
                    &MirrorSource::Download,
                    first + time::Duration::hours(hours),
                )
                .unwrap(),
        );
    }
}

/// Puts back `settings.toml` holding only `names`, as a user who removed the others by hand would.
fn hold_only(folders: &Folders, names: &[&str]) {
    let text: String = names
        .iter()
        .map(|name| format!("[[repositories]]\nname = \"{name}\"\n"))
        .collect();
    fs::write(folders.settings_file(), text).unwrap();
}

/// The window open on the first of `names`, with `conventions`, and the picker listing the rest below it in
/// their order.
fn opened(names: &[&str], conventions: Conventions) -> Home {
    opened_with(names, conventions, Filter::starters())
}

/// As `opened`, for a first Repository written by hand without its Filters, which reads them switched off.
fn opened_by_hand(names: &[&str], conventions: Conventions) -> Home {
    opened_with(names, conventions, Filter::left_out())
}

fn opened_with(names: &[&str], conventions: Conventions, filters: Vec<Filter>) -> Home {
    Home::Repository {
        repository: RepositorySettings {
            name: repository(names[0]),
            conventions,
            filters,
        },
        others: names[1..].iter().map(|name| repository(name)).collect(),
    }
}

/// Separate data and settings folders, as an installed Tsugiki on Windows has, and the folder around
/// them.
fn temp_folders(name: &str) -> (Folders, TempPath) {
    let root = crate::testing::temp_path(&format!("settings-{name}"));
    let folders = Folders {
        data: root.join("data"),
        settings: root.join("settings"),
        logs: root.join("data"),
    };
    fs::create_dir_all(&folders.data).unwrap();
    fs::create_dir_all(&folders.settings).unwrap();
    (folders, root)
}

/// What `settings.toml` says after Tsugiki saves `settings` over `before`.
fn saved_over(name: &str, before: &str, settings: &Settings) -> String {
    let (folders, _root) = temp_folders(name);
    fs::write(folders.settings_file(), before).unwrap();

    SettingsFiles::load(folders.clone())
        .write(settings)
        .unwrap();
    fs::read_to_string(folders.settings_file()).unwrap()
}

/// Writes `settings_path` into `machine.toml`.
fn point_at(folders: &Folders, path: &Path) {
    let path = path.to_str().unwrap().replace('\\', "/");
    fs::write(
        folders.machine_file(),
        format!("settings_path = \"{path}\"\n"),
    )
    .unwrap();
}

fn only_problem(files: &SettingsFiles) -> String {
    let problems = &files.loaded().problems;
    assert_eq!(problems.len(), 1, "{problems:?}");
    problems.first().unwrap().clone()
}

#[test]
fn with_no_files_tsugiki_starts_from_the_defaults_and_creates_nothing() {
    let (folders, _root) = temp_folders("no-files");

    let files = SettingsFiles::load(folders.clone());
    let created = folders.settings_file().exists() || folders.machine_file().exists();

    assert_eq!(files.loaded(), &LoadedSettings::default());
    assert!(files.loaded().settings.status_bar.visible);
    assert!(files.loaded().settings.releases.check);
    assert!(!created);
}

#[test]
fn the_check_for_a_new_version_turns_off_in_settings_toml() {
    let (folders, _root) = temp_folders("releases");
    fs::write(folders.settings_file(), "[releases]\ncheck = false\n").unwrap();

    let files = SettingsFiles::load(folders);

    assert!(!files.loaded().settings.releases.check);
    assert!(files.loaded().problems.is_empty());
}

#[test]
fn a_value_in_settings_toml_is_read_at_start() {
    let (folders, _root) = temp_folders("read");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, hidden());
    assert!(files.loaded().problems.is_empty());
}

#[test]
fn values_a_file_leaves_out_or_this_version_doesnt_know_change_nothing() {
    let (folders, _root) = temp_folders("unknown");
    let text = "from_a_newer_tsugiki = 1\n[status_bar]\nanother = 2\n";
    fs::write(folders.settings_file(), text).unwrap();

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded(), &LoadedSettings::default());
}

/// What `settings.toml` holding `text` loads as.
fn loaded_from(name: &str, text: &str) -> LoadedSettings {
    let (folders, _root) = temp_folders(name);
    fs::write(folders.settings_file(), text).unwrap();
    let files = SettingsFiles::load(folders);
    files.loaded().clone()
}

#[test]
fn appearance_names_the_theme_and_a_theme_for_each_side() {
    let text = r#"
[appearance]
theme = "Gruvbox Light Hard"
light_theme = "VS Code Light Modern"
dark_theme = "Gruvbox Dark Hard"
"#;
    let loaded = loaded_from("appearance", text);

    assert_eq!(
        loaded.settings.appearance,
        AppearanceSettings {
            theme: ThemeChoice::Theme(named("Gruvbox Light Hard")),
            light_theme: named("VS Code Light Modern"),
            dark_theme: named("Gruvbox Dark Hard"),
        }
    );
    assert_eq!(loaded.problems, Vec::<String>::new());
}

#[test]
fn an_appearance_key_left_out_takes_its_default() {
    let loaded = loaded_from("appearance-part", "[appearance]\ntheme = \"system\"\n");

    assert_eq!(loaded.settings.appearance, AppearanceSettings::default());
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(Settings::default().appearance, hidden().appearance);
}

/// A file from before #755 held a mode, which reads as the theme of its side.
#[test]
fn a_mode_from_before_reads_as_the_theme_its_side_names() {
    let text = "[appearance]\ntheme = \"dark\"\ndark_theme = \"Gruvbox Dark Hard\"\n";
    let dark = loaded_from("appearance-dark-mode", text);
    let light = loaded_from("appearance-light-mode", "[appearance]\ntheme = \"light\"\n");

    let appearance = &dark.settings.appearance;
    assert_eq!(appearance.theme, ThemeChoice::Side(ThemeKind::Dark));
    assert_eq!(
        light.settings.appearance.theme,
        ThemeChoice::Side(ThemeKind::Light)
    );
    assert_eq!(dark.problems, Vec::<String>::new());
    // The UI gets the mode as the file holds it.
    let written = serde_json::to_value(appearance).unwrap();
    assert_eq!(written["theme"], "dark");
}

/// The name stays as written: the `themes` command checks it against the folder (#755).
#[test]
fn a_theme_name_no_theme_has_stays_as_written() {
    let loaded = loaded_from("appearance-unknown", "[appearance]\ntheme = \"Dusk\"\n");

    assert_eq!(
        loaded.settings.appearance.theme,
        ThemeChoice::Theme(named("Dusk"))
    );
    assert_eq!(loaded.problems, Vec::<String>::new());
}

#[test]
fn the_view_says_where_the_settings_file_gives_theme() {
    let (folders, _root) = temp_folders("appearance-place");
    let file = folders.settings_file();
    let given = "[releases]\ncheck = true\n\n[appearance]\ntheme = \"Dusk\"\n";

    let none = SettingsFiles::load(folders.clone()).view().theme_place;
    fs::write(&file, "").unwrap();
    let left_out = SettingsFiles::load(folders.clone()).view().theme_place;
    fs::write(&file, given).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let place = files.view().theme_place;
    // The settings kept after a mistake came from text the file no longer holds.
    fs::write(&file, format!("\n\n{given}[appearance\n")).unwrap();
    files.reload();
    let broken = files.view();

    let path = file.display();
    assert_eq!(none, path.to_string());
    assert_eq!(left_out, path.to_string());
    assert_eq!(place, format!("{path}, line 5"));
    assert_eq!(
        broken.loaded.settings.appearance.theme,
        ThemeChoice::Theme(named("Dusk"))
    );
    assert_eq!(broken.theme_place, path.to_string());
}

#[test]
fn a_theme_or_a_theme_name_that_isnt_a_string_keeps_its_default_and_is_reported() {
    let text = r#"[status_bar]
visible = false

[appearance]
theme = 7
light_theme = ["Gruvbox Light Hard"]
dark_theme = 3
"#;
    let loaded = loaded_from("appearance-bad", text);

    assert_eq!(
        loaded.settings,
        hidden(),
        "the rest of the file still counts"
    );
    let problems: Vec<&str> = loaded
        .problems
        .iter()
        .map(|problem| problem.split_once("settings.toml").unwrap().1)
        .collect();
    assert_eq!(
        problems,
        [
            r#", line 5: theme isn't "system" or a theme's name, so Tsugiki follows the system."#,
            ", line 6: light_theme isn't a theme's name, so Tsugiki uses VS Code Light Modern.",
            ", line 7: dark_theme isn't a theme's name, so Tsugiki uses VS Code Dark Modern.",
        ]
    );
}

/// Saving would write the default over the value Tsugiki couldn't read, and the user's text would go.
#[test]
fn a_save_refuses_while_appearance_holds_a_value_tsugiki_cant_read() {
    let (folders, _root) = temp_folders("appearance-unread-save");
    let before = "[appearance]\ntheme = false\n";
    fs::write(folders.settings_file(), before).unwrap();

    let refusal = SettingsFiles::load(folders.clone()).write(&hidden());
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    let message = refusal.unwrap_err().message;
    assert!(
        message.contains(r#"settings.toml, line 2: theme isn't "system""#),
        "{message}"
    );
    assert_eq!(after, before);
}

#[test]
fn diff_files_picks_one_file_at_a_time() {
    let loaded = loaded_from("diff-one", "[diff]\nfiles = \"one\"\n");

    assert_eq!(loaded.settings.diff.files, DiffFiles::One);
    assert_eq!(loaded.problems, Vec::<String>::new());
}

#[test]
fn every_pr_opens_on_all_its_files_unless_diff_files_says_otherwise() {
    let loaded = loaded_from("diff-left-out", "[diff]\n");

    assert_eq!(loaded.settings.diff.files, DiffFiles::All);
    assert_eq!(Settings::default().diff.files, DiffFiles::All);
}

#[test]
fn a_diff_files_tsugiki_cant_read_keeps_all_files_and_is_reported() {
    let text = "[status_bar]\nvisible = false\n\n[diff]\nfiles = \"two\"\n";
    let loaded = loaded_from("diff-bad", text);

    assert_eq!(
        loaded.settings,
        hidden(),
        "the rest of the file still counts"
    );
    let problems: Vec<&str> = loaded
        .problems
        .iter()
        .map(|problem| problem.split_once("settings.toml").unwrap().1)
        .collect();
    assert_eq!(
        problems,
        [r#", line 5: files isn't "all" or "one", so each PR opens on all its files."#]
    );
}

/// As with `[appearance]`, saving would write the default over what the user wrote.
#[test]
fn a_save_refuses_while_diff_holds_a_value_tsugiki_cant_read() {
    let (folders, _root) = temp_folders("diff-unread-save");
    let before = "[diff]\nfiles = \"One\"\n";
    fs::write(folders.settings_file(), before).unwrap();

    let refusal = SettingsFiles::load(folders.clone()).write(&hidden());
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    let message = refusal.unwrap_err().message;
    assert!(
        message.contains(r#"settings.toml, line 2: files isn't "all" or "one""#),
        "{message}"
    );
    assert_eq!(after, before);
}

#[test]
fn diff_files_is_saved_only_when_it_isnt_all() {
    let one = Settings {
        diff: DiffSettings {
            files: DiffFiles::One,
            ..DiffSettings::default()
        },
        ..Settings::default()
    };

    assert_eq!(
        saved_over("diff-save", "", &one),
        "[diff]\nfiles = \"one\"\n"
    );
    assert_eq!(
        saved_over(
            "diff-save-default",
            "[diff]\nfiles = \"one\"\n",
            &Settings::default()
        ),
        ""
    );
}

#[test]
fn diff_peek_reads_inline_and_split_and_opens_inline_when_left_out() {
    let inline = loaded_from("peek-inline", "[diff]\npeek = \"inline\"\n");
    let split = loaded_from("peek-split", "[diff]\npeek = \"split\"\n");
    let left_out = loaded_from("peek-left-out", "[diff]\nfiles = \"one\"\n");

    assert_eq!(inline.settings.diff.peek, DiffPeek::Inline);
    assert_eq!(split.settings.diff.peek, DiffPeek::Split);
    assert_eq!(left_out.settings.diff.peek, DiffPeek::Inline);
    assert_eq!(Settings::default().diff.peek, DiffPeek::Inline);
    for loaded in [inline, split, left_out] {
        assert_eq!(loaded.problems, Vec::<String>::new());
    }
}

#[test]
fn a_diff_peek_tsugiki_cant_read_opens_inline_and_is_reported() {
    let text = "[status_bar]\nvisible = false\n\n[diff]\npeek = \"below\"\n";
    let loaded = loaded_from("peek-bad", text);

    assert_eq!(
        loaded.settings,
        hidden(),
        "the rest of the file still counts"
    );
    let problems: Vec<&str> = loaded
        .problems
        .iter()
        .map(|problem| problem.split_once("settings.toml").unwrap().1)
        .collect();
    assert_eq!(
        problems,
        [
            r#", line 5: peek isn't "inline" or "split", so the Definition peek opens under its line."#
        ]
    );
}

#[test]
fn diff_peek_is_saved_only_when_it_isnt_inline() {
    let split = Settings {
        diff: DiffSettings {
            peek: DiffPeek::Split,
            ..DiffSettings::default()
        },
        ..Settings::default()
    };

    assert_eq!(
        saved_over("peek-save", "", &split),
        "[diff]\npeek = \"split\"\n"
    );
    assert_eq!(
        saved_over(
            "peek-save-default",
            "[diff]\npeek = \"split\"\n",
            &Settings::default()
        ),
        ""
    );
}

#[test]
fn keys_gives_actions_other_keys_or_none_in_the_files_order() {
    let text = "[keys]\nlocal_mark = \"\"\narchive = \"a\"\npin = \"Ctrl+J P\"\n";
    let loaded = loaded_from("keys", text);

    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(
        serde_json::to_string(&loaded.settings.keys).unwrap(),
        r#"{"local_mark":"","archive":"a","pin":"ctrl+j p"}"#
    );
    assert_eq!(Settings::default().keys, Remaps::default());
}

#[test]
fn a_keys_line_tsugiki_cant_read_keeps_its_actions_default_and_is_reported() {
    let text = "[status_bar]\nvisible = false\n\n[keys]\narchive = \"tab\"\nfrobnicate = \"f\"\nmute = 3\n";
    let loaded = loaded_from("keys-bad", text);

    assert_eq!(
        loaded.settings.status_bar,
        hidden().status_bar,
        "the rest of the file still counts"
    );
    assert_eq!(serde_json::to_string(&loaded.settings.keys).unwrap(), "{}");
    let problems: Vec<&str> = loaded
        .problems
        .iter()
        .map(|problem| problem.split_once("settings.toml").unwrap().1)
        .collect();
    assert_eq!(
        problems,
        [
            r#", line 5: archive gives "tab", but without Ctrl or Alt that key keeps its own work, so Archive keeps "e"."#,
            ", line 6: frobnicate isn't an action Tsugiki has, so Tsugiki skips the line.",
            r#", line 7: mute needs a key in quotes, such as "ctrl+k m", so Mute keeps "m"."#,
        ]
    );
}

/// As with `[appearance]`, a `keys` that isn't a table fails the file.
#[test]
fn a_keys_that_isnt_a_table_fails_the_file() {
    let failed = loaded_from("keys-not-a-table", "keys = \"e\"\n");

    let problem = failed.problems.first().cloned().unwrap_or_default();
    assert!(
        problem.contains("settings.toml, line 1")
            && problem.contains("a table of actions and their keys"),
        "{problem}"
    );
    assert_eq!(failed.settings, Settings::default());
}

/// The Settings dialog doesn't edit `[keys]` yet, so a save leaves it as the user wrote it, a line Tsugiki
/// can't read included: nothing would write over that line.
#[test]
fn a_save_leaves_keys_as_the_user_wrote_them() {
    let before = "[keys]\nlocal_mark = \"Ctrl+K M\"\narchive = \"tab\"\n";

    assert_eq!(
        saved_over("keys-save", before, &hidden()),
        format!("{before}\n[status_bar]\nvisible = false\n")
    );
}

#[test]
fn appearance_is_saved_by_name_and_only_where_it_differs_from_the_default() {
    let settings = Settings {
        appearance: AppearanceSettings {
            theme: ThemeChoice::Theme(named("Gruvbox Dark Hard")),
            light_theme: named("Gruvbox Light Hard"),
            dark_theme: named("VS Code Dark Modern"),
        },
        ..Settings::default()
    };
    let after = saved_over("appearance-save", "", &settings);

    assert_eq!(
        after,
        "[appearance]\ntheme = \"Gruvbox Dark Hard\"\nlight_theme = \"Gruvbox Light Hard\"\n"
    );
}

/// A file from before #755 held a mode. Only the `themes` folder can say which theme the side's name gives,
/// as its file may be gone, so the Settings dialog's Apply keeps the mode and what the window shows.
#[test]
fn an_apply_keeps_a_mode_from_before() {
    let before = "[appearance]\ntheme = \"dark\" # by night\ndark_theme = \"Night\"\n";
    let (folders, _root) = temp_folders("appearance-mode-save");
    fs::write(folders.settings_file(), before).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let mut settings = files.loaded().settings.clone();
    settings.status_bar.visible = false;
    files.write(&settings).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(after, format!("{before}\n[status_bar]\nvisible = false\n"));
}

/// The theme picker saves `theme` alone (#756). A mode from before (#755) gives way to the Theme picked,
/// whichever side it is for.
#[test]
fn a_pick_writes_the_theme_picked_in_place_of_a_mode() {
    let (folders, _root) = temp_folders("appearance-mode-picked");
    let file = folders.settings_file();
    let mut files = SettingsFiles::load(folders.clone());

    fs::write(&file, "[appearance]\ntheme = \"light\"\n").unwrap();
    files
        .save_theme(&ThemeChoice::Theme(named("Gruvbox Dark Hard")))
        .unwrap();
    let after = fs::read_to_string(&file).unwrap();

    assert_eq!(after, "[appearance]\ntheme = \"Gruvbox Dark Hard\"\n");
}

/// The theme picker's Enter (#518, #756) changes `theme` and leaves `light_theme`, `dark_theme`, the user's
/// comments and the other keys as they were. A pick of a dark Theme saves its name whichever side the system
/// is on, which the core doesn't ask.
#[test]
fn a_picked_theme_is_saved_to_theme_alone() {
    let (folders, _root) = temp_folders("appearance-picked");
    let before = "# Mine\n[releases]\ncheck = true\n\n[appearance]\n\
                  light_theme = \"Gruvbox Light Hard\" # by day\ndark_theme = \"VS Code Dark Modern\"\n";
    fs::write(folders.settings_file(), before).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files
        .save_theme(&ThemeChoice::Theme(named("Gruvbox Dark Hard")))
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let loaded = files.loaded().settings.appearance.clone();

    assert_eq!(
        after,
        format!("{before}theme = \"Gruvbox Dark Hard\"\n"),
        "the default `check` and `dark_theme` stay as the user wrote them"
    );
    assert_eq!(
        loaded,
        AppearanceSettings {
            theme: ThemeChoice::Theme(named("Gruvbox Dark Hard")),
            light_theme: named("Gruvbox Light Hard"),
            dark_theme: named("VS Code Dark Modern"),
        }
    );
}

/// Follow system is `theme`'s default, so its pick takes the key out of the file, unless the user wrote a
/// comment on it, which keeps it as `"system"`.
#[test]
fn a_pick_of_follow_system_saves_theme_as_system() {
    let (folders, _root) = temp_folders("appearance-picked-system");
    let file = folders.settings_file();
    let mut files = SettingsFiles::load(folders.clone());

    let plain = "[appearance]\ntheme = \"Gruvbox Dark Hard\"\ndark_theme = \"Gruvbox Dark Hard\"\n";
    fs::write(&file, plain).unwrap();
    files.save_theme(&ThemeChoice::System).unwrap();
    let without_comment = fs::read_to_string(&file).unwrap();
    let loaded = files.loaded().settings.appearance.theme.clone();
    fs::write(
        &file,
        "[appearance]\ntheme = \"Gruvbox Dark Hard\" # mine\n",
    )
    .unwrap();
    files.save_theme(&ThemeChoice::System).unwrap();
    let with_comment = fs::read_to_string(&file).unwrap();

    assert_eq!(
        without_comment,
        "[appearance]\ndark_theme = \"Gruvbox Dark Hard\"\n"
    );
    assert_eq!(loaded, ThemeChoice::System);
    assert_eq!(with_comment, "[appearance]\ntheme = \"system\" # mine\n");
}

#[test]
fn a_picked_theme_isnt_saved_over_a_settings_file_that_doesnt_parse() {
    let (folders, _root) = temp_folders("appearance-picked-broken");
    let before = "[appearance\n";
    fs::write(folders.settings_file(), before).unwrap();

    let refusal = SettingsFiles::load(folders.clone())
        .save_theme(&ThemeChoice::Theme(named("Gruvbox Light Hard")));
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert!(
        refusal.unwrap_err().message.contains("settings.toml"),
        "names the file"
    );
    assert_eq!(after, before);
}

#[test]
fn the_owner_saves_a_picked_theme_and_answers_with_it() {
    let (folders, _root) = temp_folders("owner-theme");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

    let answer = owner.save_theme(ThemeChoice::Theme(named("Gruvbox Light Hard")));
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    let appearance = answer.unwrap().loaded.settings.appearance;
    assert_eq!(
        appearance.theme,
        ThemeChoice::Theme(named("Gruvbox Light Hard"))
    );
    assert_eq!(owner.view().loaded.settings.appearance, appearance);
    assert_eq!(after, "[appearance]\ntheme = \"Gruvbox Light Hard\"\n");
}

#[test]
fn the_owner_reads_a_hand_edit_again_as_the_settings_dialog_opens() {
    let (folders, _root) = temp_folders("owner-reread");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();

    let before = owner.view();
    let answer = owner.reread();

    assert!(before.loaded.settings.status_bar.visible);
    let loaded = answer.unwrap().loaded;
    assert_eq!(loaded.settings, hidden());
    assert_eq!(owner.view().loaded, loaded);
}

#[test]
fn the_owner_saves_what_apply_keeps_and_a_restart_reads_it() {
    let (folders, _root) = temp_folders("owner-apply");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

    let answer = owner.apply(SettingsChanges {
        status_bar: Some(StatusBarSettings { visible: false }),
        ..SettingsChanges::default()
    });
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let restarted = SettingsFiles::load(folders);

    assert_eq!(answer.unwrap().loaded.settings, hidden());
    assert_eq!(owner.view().loaded.settings, hidden());
    assert_eq!(after, "[status_bar]\nvisible = false\n");
    assert_eq!(restarted.loaded().settings, hidden());
}

#[test]
fn apply_keeps_a_hand_edit_made_while_the_dialog_was_open_to_what_it_didnt_change() {
    let (folders, _root) = temp_folders("apply-keeps-hand-edit");
    let mut files = SettingsFiles::load(folders.clone());
    // While the dialog is open, the user edits other tables by hand, and one the dialog changes too.
    fs::write(
        folders.settings_file(),
        "[diff]\nfiles = \"one\"\n\n[releases]\ncheck = false\n\n[status_bar]\nvisible = false\n",
    )
    .unwrap();

    let applied = files.apply(SettingsChanges {
        status_bar: Some(StatusBarSettings { visible: true }),
        ..SettingsChanges::default()
    });
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    applied.unwrap();
    assert_eq!(
        after,
        "[diff]\nfiles = \"one\"\n\n[releases]\ncheck = false\n"
    );
    let settings = &files.loaded().settings;
    assert_eq!(settings.diff.files, DiffFiles::One);
    assert!(!settings.releases.check);
    assert!(settings.status_bar.visible);
}

#[test]
fn apply_saves_a_changed_light_theme_and_leaves_theme_alone() {
    let (folders, _root) = temp_folders("apply-light-theme");
    let text =
        "[appearance]\ntheme = \"dark\" # From before.\ndark_theme = \"Gruvbox Dark Hard\"\n";
    fs::write(folders.settings_file(), text).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let applied = files.apply(SettingsChanges {
        appearance: Some(AppearanceChanges {
            light_theme: Some(named("Gruvbox Light Hard")),
            ..AppearanceChanges::default()
        }),
        ..SettingsChanges::default()
    });
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    applied.unwrap();
    assert_eq!(
        after,
        "[appearance]\ntheme = \"dark\" # From before.\ndark_theme = \"Gruvbox Dark Hard\"\nlight_theme = \"Gruvbox Light Hard\"\n"
    );
    let appearance = &files.loaded().settings.appearance;
    assert_eq!(appearance.theme, ThemeChoice::Side(ThemeKind::Dark));
    assert_eq!(appearance.light_theme, named("Gruvbox Light Hard"));
}

#[test]
fn apply_saves_a_changed_dark_theme() {
    let (folders, _root) = temp_folders("apply-dark-theme");
    let mut files = SettingsFiles::load(folders.clone());

    files
        .apply(SettingsChanges {
            appearance: Some(AppearanceChanges {
                dark_theme: Some(named("Gruvbox Dark Hard")),
                ..AppearanceChanges::default()
            }),
            ..SettingsChanges::default()
        })
        .unwrap();

    let after = fs::read_to_string(folders.settings_file()).unwrap();
    assert_eq!(after, "[appearance]\ndark_theme = \"Gruvbox Dark Hard\"\n");
    let appearance = &files.loaded().settings.appearance;
    assert_eq!(appearance.dark_theme, named("Gruvbox Dark Hard"));
    assert_eq!(appearance.theme, ThemeChoice::System);
}

#[test]
fn apply_saves_the_theme_picked_and_takes_following_the_system_out_of_the_file() {
    let (folders, _root) = temp_folders("apply-theme");
    let mut files = SettingsFiles::load(folders.clone());
    let pick = |theme| AppearanceChanges {
        theme: Some(theme),
        ..AppearanceChanges::default()
    };

    files
        .apply(SettingsChanges {
            appearance: Some(pick(ThemePick::Named {
                name: named("Gruvbox Dark Hard"),
            })),
            ..SettingsChanges::default()
        })
        .unwrap();
    let named_after = fs::read_to_string(folders.settings_file()).unwrap();
    files
        .apply(SettingsChanges {
            appearance: Some(pick(ThemePick::System)),
            ..SettingsChanges::default()
        })
        .unwrap();
    let system_after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(named_after, "[appearance]\ntheme = \"Gruvbox Dark Hard\"\n");
    assert_eq!(system_after, "");
    assert_eq!(
        files.loaded().settings.appearance.theme,
        ThemeChoice::System
    );
}

#[test]
fn apply_refuses_a_theme_picked_by_a_name_theme_reads_otherwise_and_saves_nothing() {
    for name in ["system", "light", "dark"] {
        let (folders, _root) = temp_folders(&format!("apply-reserved-{name}"));
        let mut files = SettingsFiles::load(folders.clone());

        let applied = files.apply(SettingsChanges {
            status_bar: Some(StatusBarSettings { visible: false }),
            appearance: Some(AppearanceChanges {
                theme: Some(ThemePick::Named { name: named(name) }),
                ..AppearanceChanges::default()
            }),
            ..SettingsChanges::default()
        });

        let error = applied.unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound, "{name}");
        assert_eq!(
            error.message,
            format!("No theme is named {name:?}."),
            "{name}"
        );
        assert!(!folders.settings_file().exists(), "{name}");
    }
}

#[test]
fn a_view_s_themes_show_the_appearance_asked_for_by_the_rules_the_file_s_go_by() {
    let (folders, _root) = temp_folders("view-themes");
    fs::write(
        folders.settings_file(),
        "[appearance]\ndark_theme = \"Gruvbox Dark Hard\"\n",
    )
    .unwrap();
    let view = SettingsFiles::load(folders).view();
    let preview = AppearanceSettings {
        theme: ThemeChoice::Theme(named("Gruvbox Light Hard")),
        light_theme: named("Solarized Light"),
        dark_theme: named("VS Code Dark Modern"),
    };

    let from_file = view.themes(&view.loaded.settings.appearance).unwrap();
    let previewed = view.themes(&preview).unwrap();

    assert_eq!(from_file.theme, ThemeInForce::System);
    assert_eq!(from_file.dark.chosen, "Gruvbox Dark Hard");
    assert_eq!(
        previewed.theme,
        ThemeInForce::Named {
            name: named("Gruvbox Light Hard"),
            side: ThemeKind::Light,
        }
    );
    assert_eq!(previewed.light.chosen, "VS Code Light Modern");
    assert_eq!(previewed.dark.chosen, "VS Code Dark Modern");
}

#[test]
fn apply_of_another_change_keeps_a_side_s_name_no_theme_has() {
    let (folders, _root) = temp_folders("apply-keeps-missing-theme");
    let text = "[appearance]\ndark_theme = \"Solarized Dark\"\n";
    fs::write(folders.settings_file(), text).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let applied = files.apply(SettingsChanges {
        appearance: Some(AppearanceChanges {
            theme: Some(ThemePick::Named {
                name: named("Gruvbox Light Hard"),
            }),
            ..AppearanceChanges::default()
        }),
        ..SettingsChanges::default()
    });
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    applied.unwrap();
    assert_eq!(
        after,
        "[appearance]\ndark_theme = \"Solarized Dark\"\ntheme = \"Gruvbox Light Hard\"\n"
    );
}

#[test]
fn apply_over_a_settings_file_that_stopped_parsing_refuses_and_leaves_it_alone() {
    let (folders, _root) = temp_folders("apply-broken");
    let mut files = SettingsFiles::load(folders.clone());
    fs::write(folders.settings_file(), "[status_bar\n").unwrap();

    let applied = files.apply(SettingsChanges {
        releases: Some(ReleaseSettings { check: false }),
        ..SettingsChanges::default()
    });
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert!(applied.is_err());
    assert_eq!(after, "[status_bar\n");
}

#[test]
fn a_file_that_doesnt_parse_is_left_alone_and_named_with_its_line() {
    for (name, text) in [
        ("broken", "# Mine.\n[status_bar]\nvisible = = false\n"),
        ("wrong-type", "\n[status_bar]\nvisible = \"no\"\n"),
    ] {
        let (folders, _root) = temp_folders(name);
        fs::write(folders.settings_file(), text).unwrap();

        let files = SettingsFiles::load(folders.clone());
        let after = fs::read_to_string(folders.settings_file()).unwrap();

        assert_eq!(after, text, "{name}");
        assert_eq!(files.loaded().settings, Settings::default(), "{name}");
        let problem = only_problem(&files);
        assert!(
            problem.contains("settings.toml, line 3"),
            "{name}: {problem}"
        );
    }
}

#[test]
fn a_file_it_cant_read_is_a_problem_that_names_it() {
    let (folders, _root) = temp_folders("unreadable");
    fs::create_dir(folders.settings_file()).unwrap();

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, Settings::default());
    let problem = only_problem(&files);
    assert!(problem.contains("settings.toml"), "{problem}");
}

#[test]
fn a_reload_of_a_broken_file_keeps_the_last_good_values_until_it_is_fixed() {
    let (folders, _root) = temp_folders("last-good");
    let file = folders.settings_file();
    fs::write(&file, "[status_bar]\nvisible = false\n").unwrap();
    let mut files = SettingsFiles::load(folders);

    fs::write(&file, "[status_bar\n").unwrap();
    files.reload();
    let while_broken = files.loaded().clone();

    fs::write(&file, "[status_bar]\nvisible = true\n").unwrap();
    files.reload();

    assert_eq!(while_broken.settings, hidden());
    assert_eq!(while_broken.problems.len(), 1);
    assert_eq!(files.loaded(), &LoadedSettings::default());
}

#[test]
fn a_settings_toml_removed_from_its_usual_folder_means_the_defaults_again() {
    let (folders, _root) = temp_folders("removed");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    fs::remove_file(folders.settings_file()).unwrap();
    files.reload();

    assert_eq!(files.loaded(), &LoadedSettings::default());
}

#[test]
fn an_empty_list_of_filters_stays_in_a_repositorys_table() {
    let table = Table::from_iter([(FILTERS, toml_edit::value(toml_edit::Array::new()))]);

    let kept = with_filters_as_tables(table);

    assert_eq!(kept[FILTERS].as_array().unwrap().len(), 0);
}

#[test]
fn a_repositorys_table_with_no_filters_gains_none() {
    let kept = with_filters_as_tables(Table::new());
    assert!(kept.is_empty());
}

/// The one Repository `text` holds, as a read of the file gives it.
fn the_repository_in(name: &str, text: &str) -> RepositorySettings {
    let (folders, _root) = temp_folders(name);
    fs::write(folders.settings_file(), text).unwrap();
    let files = SettingsFiles::load(folders);
    assert_eq!(files.loaded().problems, Vec::<String>::new());
    files.loaded().settings.repositories[0].clone()
}

fn filter(name: &str, query: &str) -> Filter {
    Filter {
        name: name.to_owned().try_into().unwrap(),
        query: query.to_owned().try_into().unwrap(),
        on_workbench: OnWorkbench::Off,
    }
}

fn query(text: &str) -> FilterQuery {
    text.to_owned().try_into().unwrap()
}

#[test]
fn adding_a_repository_writes_its_starter_filters_where_the_user_can_edit_them() {
    let (folders, _root) = temp_folders("starter-filters");
    let mut files = SettingsFiles::load(folders.clone());

    drop(
        files
            .add_repository(
                &RepositorySettings::new(repository("a/b")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let written = fs::read_to_string(folders.settings_file()).unwrap();
    let read = files.loaded().settings.repositories[0].clone();

    assert!(written.contains("[[repositories.filters]]"), "{written}");
    assert!(written.contains(r#"name = "Mine""#), "{written}");
    assert!(
        written.contains(r#"query = "is:open author:@me""#),
        "{written}"
    );
    assert!(written.contains("on_workbench = true"), "{written}");
    assert_eq!(read.filters, Filter::starters());
}

#[test]
fn a_repository_with_no_filters_in_the_file_gets_the_starters_switched_off() {
    let read = the_repository_in("no-filters", "[[repositories]]\nname = \"a/b\"\n");
    assert_eq!(read.filters, Filter::left_out());
    let names: Vec<String> = (read.filters.iter())
        .map(|filter| filter.name.clone().into())
        .collect();
    assert_eq!(names, ["Mine", "Involved"]);
    assert!(
        (read.filters.iter()).all(|filter| filter.on_workbench == OnWorkbench::Off),
        "{:?}",
        read.filters
    );
}

#[test]
fn of_two_filters_with_the_same_search_the_settings_read_the_first() {
    let read = the_repository_in(
        "same-search",
        "[[repositories]]\nname = \"a/b\"\n
[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\n
[[repositories.filters]]\nname = \"Docs\"\nquery = \"label:docs\"\n
[[repositories.filters]]\nname = \"Bug triage\"\nquery = \" label:bug \"\non_workbench = true\n",
    );
    let switches: Vec<(String, bool)> = (read.filters.into_iter())
        .map(|filter| (filter.name.into(), filter.on_workbench.into()))
        .collect();
    assert_eq!(
        switches,
        [("Bugs".to_owned(), false), ("Docs".to_owned(), false)]
    );
}

#[test]
fn a_filter_reads_its_switch_and_one_without_it_is_off_the_workbench() {
    let read = the_repository_in(
        "filter-switch",
        "[[repositories]]\nname = \"a/b\"\n
[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\non_workbench = true\n
[[repositories.filters]]\nname = \"Docs\"\nquery = \"label:docs\"\n",
    );
    let switches: Vec<(String, bool)> = (read.filters.into_iter())
        .map(|filter| (filter.name.into(), filter.on_workbench.into()))
        .collect();
    assert_eq!(
        switches,
        [("Bugs".to_owned(), true), ("Docs".to_owned(), false)]
    );
}

/// Files whose `settings.toml` says `text`, and the folders they live in.
fn settings_saying(name: &str, text: &str) -> (SettingsFiles, Folders, TempPath) {
    let (folders, root) = temp_folders(name);
    fs::write(folders.settings_file(), text).unwrap();
    let files = SettingsFiles::load(folders.clone());
    assert_eq!(files.loaded().problems, Vec::<String>::new());
    (files, folders, root)
}

/// Each Filter of the first Repository the files read, with its switch.
fn switches(files: &SettingsFiles) -> Vec<(String, bool)> {
    let filters = files.loaded().settings.repositories[0].filters.iter();
    (filters.map(|filter| (filter.name.clone().into(), filter.on_workbench.into()))).collect()
}

#[test]
fn a_switch_puts_the_first_filter_with_that_search_on_the_workbench_and_keeps_the_users_words() {
    let (mut files, folders, _root) = settings_saying(
        "switch-on",
        "\
[[repositories]]
name = \"a/b\"

# Mine.
[[repositories.filters]]
name = \"Bugs\"
query = \" label:bug \" # The bugs.

[[repositories.filters]]
name = \"Docs\"
query = \"label:docs\"
on_workbench = true

[[repositories.filters]]
name = \"Bugs again\"
query = \"label:bug\"
on_workbench = false # Not yet.
",
    );

    files
        .put_on_workbench(&repository("A/B"), &query("label:bug"), OnWorkbench::On)
        .unwrap();
    files
        .put_on_workbench(&repository("a/b"), &query("label:docs"), OnWorkbench::Off)
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(
        after,
        "\
[[repositories]]
name = \"a/b\"

# Mine.
[[repositories.filters]]
name = \"Bugs\"
query = \" label:bug \" # The bugs.
on_workbench = true

[[repositories.filters]]
name = \"Docs\"
query = \"label:docs\"
on_workbench = false

[[repositories.filters]]
name = \"Bugs again\"
query = \"label:bug\"
on_workbench = false # Not yet.
"
    );
    assert_eq!(
        switches(&files),
        [("Bugs".to_owned(), true), ("Docs".to_owned(), false)]
    );
}

#[test]
fn a_switch_in_a_list_written_inline_stays_inline() {
    let (mut files, folders, _root) = settings_saying(
        "switch-inline",
        "repositories = [{ name = \"a/b\", filters = [{ name = \"Bugs\", query = \"label:bug\" }] }]\n",
    );

    files
        .put_on_workbench(&repository("a/b"), &query("label:bug"), OnWorkbench::On)
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(
        after,
        "repositories = [{ name = \"a/b\", filters = [{ name = \"Bugs\", query = \"label:bug\", on_workbench = true }] }]\n"
    );
    assert_eq!(switches(&files), [("Bugs".to_owned(), true)]);
}

#[test]
fn a_switch_on_a_repository_without_filters_writes_out_the_starters_it_reads_as() {
    let (mut files, folders, _root) =
        settings_saying("switch-starters", "[[repositories]]\nname = \"a/b\"\n");

    files
        .put_on_workbench(
            &repository("a/b"),
            &query("is:open involves:@me -author:@me"),
            OnWorkbench::On,
        )
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(
        after,
        "[[repositories]]\nname = \"a/b\"\n
[[repositories.filters]]\nname = \"Mine\"\nquery = \"is:open author:@me\"\non_workbench = false\n
[[repositories.filters]]\nname = \"Involved\"\nquery = \"is:open involves:@me -author:@me\"\non_workbench = true\n"
    );
    assert_eq!(
        switches(&files),
        [("Mine".to_owned(), false), ("Involved".to_owned(), true)]
    );
}

#[test]
fn a_switch_refuses_a_repository_or_a_search_the_file_doesnt_hold_and_saves_nothing() {
    let before = "[[repositories]]\nname = \"a/b\"\nfilters = []\n";
    let (mut files, folders, _root) = settings_saying("switch-refused", before);

    let no_repository = files
        .put_on_workbench(&repository("c/d"), &query("label:bug"), OnWorkbench::On)
        .unwrap_err();
    let no_search = files
        .put_on_workbench(&repository("a/b"), &query("label:bug"), OnWorkbench::On)
        .unwrap_err();

    assert_eq!(no_repository.kind, ErrorKind::NotFound);
    assert_eq!(no_repository.message, "Tsugiki doesn't have c/d.");
    assert_eq!(no_search.kind, ErrorKind::NotFound);
    assert_eq!(
        no_search.message,
        "a/b has no Filter that searches label:bug."
    );
    assert_eq!(fs::read_to_string(folders.settings_file()).unwrap(), before);
}

#[test]
fn the_owner_saves_a_switch_and_answers_with_what_the_files_say() {
    let (files, _folders, _root) = settings_saying(
        "owner-switch",
        "[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\n",
    );
    let owner = SettingsOwner::start(Ok(files));

    let saved = owner
        .put_on_workbench(repository("a/b"), query("label:bug"), OnWorkbench::On)
        .unwrap();

    assert_eq!(
        saved.loaded.settings.repositories[0].filters[0].on_workbench,
        OnWorkbench::On
    );
    assert_eq!(owner.view(), saved);
}

/// A save the session heard of, in brief: the Repository, the Conventions and the first Filter's switch
/// it saved, those it replaced, and what saved them.
type Told = (
    String,
    (Conventions, OnWorkbench),
    Option<(Conventions, OnWorkbench)>,
    SavedBy,
);

/// Has `owner` hand the session each save, which the receiver hears in brief.
fn telling_session(owner: &SettingsOwner) -> mpsc::Receiver<Told> {
    let (sent, incoming) = mpsc::channel();
    let said =
        |settings: &RepositorySettings| (settings.conventions, settings.filters[0].on_workbench);
    owner.tell_session(Box::new(move |save: SettingsSave| {
        let replaced = save.replaced().map(said);
        let saved = save.saved();
        sent.send((saved.name.to_string(), said(saved), replaced, save.by()))
            .unwrap();
    }));
    incoming
}

#[test]
fn a_repository_the_files_drop_and_list_again_comes_back_with_the_settings_it_had() {
    let bugs = |switch: &str| {
        format!("\n[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\n{switch}")
    };
    let a_b = format!("[[repositories]]\nname = \"a/b\"\n{}", bugs(""));
    let c_d = |switch| format!("\n[[repositories]]\nname = \"c/d\"\n{}", bugs(switch));
    let (files, folders, _root) =
        settings_saying("owner-tells-return", &format!("{a_b}{}", c_d("")));
    let owner = SettingsOwner::start(Ok(files));
    let incoming = telling_session(&owner);

    // A hand edit takes c/d out, an editor's save leaves no file for a moment, and a hand edit puts c/d
    // back with its Filter switched on.
    fs::write(folders.settings_file(), &a_b).unwrap();
    drop(owner.reread().unwrap());
    fs::remove_file(folders.settings_file()).unwrap();
    let emptied = owner.reread().unwrap();
    let back = format!("{a_b}{}", c_d("on_workbench = true\n"));
    fs::write(folders.settings_file(), back).unwrap();
    drop(owner.reread().unwrap());
    drop(owner);

    assert_eq!(emptied.loaded.settings.repositories, []);
    // a/b came back as it was, so the session hears nothing of it.
    assert_eq!(
        incoming.try_iter().collect::<Vec<_>>(),
        [(
            "c/d".to_owned(),
            (Conventions::Github, OnWorkbench::On),
            Some((Conventions::Github, OnWorkbench::Off)),
            SavedBy::HandEdit
        )]
    );
}

#[test]
fn of_a_repository_written_twice_the_session_hears_of_the_first() {
    let a_b = |switch: &str| {
        format!(
            "\n[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\n\
             query = \"label:bug\"\n{switch}"
        )
    };
    let on = "on_workbench = true\n";
    let (files, folders, _root) =
        settings_saying("owner-tells-twice", &format!("{}{}", a_b(""), a_b(on)));
    let owner = SettingsOwner::start(Ok(files));
    let incoming = telling_session(&owner);

    // Each reload reads both, and the second would undo what the first said.
    drop(owner.reread().unwrap());
    drop(owner.reread().unwrap());
    fs::write(folders.settings_file(), format!("{}{}", a_b(on), a_b(""))).unwrap();
    drop(owner.reread().unwrap());
    drop(owner.reread().unwrap());
    drop(owner);

    assert_eq!(
        incoming.try_iter().collect::<Vec<_>>(),
        [(
            "a/b".to_owned(),
            (Conventions::Github, OnWorkbench::On),
            Some((Conventions::Github, OnWorkbench::Off)),
            SavedBy::HandEdit
        )]
    );
}

#[test]
fn the_owner_hands_the_session_each_repository_s_settings_a_change_saves_otherwise_in_save_order() {
    let (files, folders, _root) = settings_saying(
        "owner-tells-session",
        "[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\n",
    );
    let owner = SettingsOwner::start(Ok(files));
    let incoming = telling_session(&owner);

    drop(owner.save_panes(PaneLayout::default()).unwrap());
    drop(
        owner
            .put_on_workbench(repository("a/b"), query("label:bug"), OnWorkbench::On)
            .unwrap(),
    );
    let file = fs::read_to_string(folders.settings_file()).unwrap();
    let edited = file.replace(
        "name = \"a/b\"\n",
        "name = \"a/b\"\nconventions = \"cpython\"\n",
    );
    fs::write(folders.settings_file(), &edited).unwrap();
    // A change that saves nothing of the Repository takes the hand edit in, and the reread after finds
    // nothing new.
    drop(owner.save_panes(PaneLayout::default()).unwrap());
    drop(owner.reread().unwrap());
    drop(owner);

    let told: Vec<_> = incoming.try_iter().collect();
    assert_ne!(edited, file, "{file}");
    // The first change replaced the settings the files held at launch, which the session never heard.
    assert_eq!(
        told,
        [
            (
                "a/b".to_owned(),
                (Conventions::Github, OnWorkbench::On),
                Some((Conventions::Github, OnWorkbench::Off)),
                SavedBy::Window
            ),
            (
                "a/b".to_owned(),
                (Conventions::Cpython, OnWorkbench::On),
                Some((Conventions::Github, OnWorkbench::On)),
                SavedBy::HandEdit
            ),
        ]
    );
}

#[test]
fn a_repository_can_say_it_wants_no_filters() {
    let read = the_repository_in(
        "empty-filters",
        "[[repositories]]\nname = \"a/b\"\nfilters = []\n",
    );
    assert_eq!(read.filters, []);
}

#[test]
fn a_filter_edited_by_hand_is_what_the_next_read_finds() {
    let read = the_repository_in(
        "edited-filter",
        "[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\n\
             query = \"label:bug\"\n",
    );
    assert_eq!(read.filters, [filter("Bugs", "label:bug")]);
}

#[test]
fn a_blank_filter_query_is_a_problem_with_the_file_and_the_last_values_stay() {
    let (folders, _root) = temp_folders("blank-filter");
    fs::write(
        folders.settings_file(),
        "[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\n\
             query = \" \"\n",
    )
    .unwrap();
    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, Settings::default());
    assert!(files.loaded().problems[0].contains("A Filter's query can't be blank."));
}

#[test]
fn a_repositorys_settings_come_from_the_list_whatever_the_case_of_its_name() {
    let chosen = RepositorySettings {
        name: cpython(),
        conventions: Conventions::Github,
        filters: Vec::new(),
    };
    let settings = Settings {
        repositories: vec![chosen.clone()],
        ..Settings::default()
    };

    let shouting = RepositoryName::parse("Python/CPython").unwrap();
    assert_eq!(settings.repository(shouting), chosen);
}

/// Settings that hold `names`, each as written.
fn holding(names: &[&str]) -> Settings {
    Settings {
        repositories: names
            .iter()
            .map(|&name| RepositorySettings::new(repository(name)))
            .collect(),
        ..Settings::default()
    }
}

/// What GitHub says adding a small Repository it calls `name` would take.
fn found(name: &str) -> RepositoryCheck {
    RepositoryCheck::new(FoundRepository {
        name: repository(name),
        counts: RepositoryCounts {
            open_issues: 10,
            open_prs: 3,
            updated_in_window: 43,
            closed_in_window: 35,
            disk_kb: 0,
        },
    })
}

#[test]
fn a_name_the_settings_hold_in_any_case_is_held_by_their_name_and_github_isnt_asked() {
    let settings = holding(&["python/cpython", "python/peps"]);

    let checked = settings.check_name(repository("Python/CPython"), &mut |_| {
        panic!("GitHub was asked about a Repository Tsugiki has")
    });

    assert_eq!(
        checked.unwrap(),
        NameCheck::Held {
            name: repository("python/cpython")
        }
    );
}

#[test]
fn a_name_the_settings_dont_hold_gets_githubs_check() {
    let settings = holding(&["python/cpython"]);
    let mut asked = Vec::new();

    let checked = settings.check_name(repository("python/peps"), &mut |name| {
        asked.push(name.to_string());
        Ok(found("python/peps"))
    });

    assert_eq!(asked, ["python/peps"]);
    assert_eq!(
        checked.unwrap(),
        NameCheck::New {
            check: found("python/peps")
        }
    );
}

#[test]
fn a_name_github_answers_with_that_the_settings_hold_is_held() {
    // GitHub follows a renamed Repository to its new name.
    let settings = holding(&["python/cpython"]);

    let checked = settings.check_name(repository("python/old-cpython"), &mut |_| {
        Ok(found("python/CPython"))
    });

    assert_eq!(
        checked.unwrap(),
        NameCheck::Held {
            name: repository("python/cpython")
        }
    );
}

#[test]
fn a_check_github_refuses_says_why() {
    let refused = Error::new(ErrorKind::NotFound, "GitHub has no python/no-such.");

    let checked =
        holding(&[]).check_name(repository("python/no-such"), &mut |_| Err(refused.clone()));

    assert_eq!(checked.unwrap_err(), refused);
}

#[test]
fn a_repository_the_user_hasnt_added_gets_new_settings() {
    let name = cpython();
    assert_eq!(
        Settings::default().repository(name.clone()),
        RepositorySettings::new(name)
    );
}

#[test]
fn settings_path_moves_settings_toml() {
    let (folders, root) = temp_folders("moved");
    let moved = root.join("dotfiles.toml");
    fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
    fs::write(folders.settings_file(), "[status_bar]\nvisible = true\n").unwrap();
    point_at(&folders, &moved);

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, hidden());
    assert!(files.loaded().problems.is_empty());
}

#[test]
fn a_relative_settings_path_starts_at_machine_toml() {
    let (folders, _root) = temp_folders("relative");
    fs::write(
        folders.data.join("beside.toml"),
        "[status_bar]\nvisible = false\n",
    )
    .unwrap();
    fs::write(folders.machine_file(), "settings_path = \"beside.toml\"\n").unwrap();

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, hidden());
}

/// A theme file travels with a `settings_path` synced from elsewhere (#519). The folder can change while
/// Tsugiki runs, so a name stays as written until the `themes` command checks it, and a theme file mended
/// later shows without a restart.
#[test]
fn a_theme_name_stays_as_written_and_the_themes_folder_is_beside_the_settings_file_in_use() {
    let (folders, root) = temp_folders("themes-beside");
    let dotfiles = root.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(&folders.settings).unwrap();
    let text = "[appearance]\ndark_theme = \"Night\"\nlight_theme = \"Gruvbox Dark Hard\"\n";
    fs::write(dotfiles.join("tsugiki.toml"), text).unwrap();
    point_at(&folders, &dotfiles.join("tsugiki.toml"));

    let files = SettingsFiles::load(folders);

    let loaded = files.loaded();
    assert_eq!(loaded.settings.appearance.dark_theme, named("Night"));
    assert_eq!(
        loaded.settings.appearance.light_theme,
        named("Gruvbox Dark Hard")
    );
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(files.view().themes_folder, dotfiles.join("themes"));
}

#[test]
fn a_missing_file_at_settings_path_starts_from_the_defaults_and_says_so() {
    let (folders, root) = temp_folders("moved-missing");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    point_at(&folders, &root.join("not-there.toml"));

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, Settings::default());
    let problem = only_problem(&files);
    assert!(problem.contains("not-there.toml"), "{problem}");
    assert!(problem.contains("defaults"), "{problem}");
}

#[test]
fn a_broken_machine_toml_is_named_with_its_line_and_keeps_the_last_good_path() {
    let (folders, root) = temp_folders("machine-broken");
    let moved = root.join("dotfiles.toml");
    fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
    point_at(&folders, &moved);
    let mut files = SettingsFiles::load(folders.clone());

    fs::write(folders.machine_file(), "\nsettings_path =\n").unwrap();
    files.reload();

    assert_eq!(files.loaded().settings, hidden());
    let problem = only_problem(&files);
    assert!(problem.contains("machine.toml, line 2"), "{problem}");
}

#[test]
fn saving_writes_only_what_differs_from_the_defaults() {
    let (folders, _root) = temp_folders("save-new");
    let mut files = SettingsFiles::load(folders.clone());

    files.write(&hidden()).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(after, "[status_bar]\nvisible = false\n");
    assert_eq!(files.loaded().settings, hidden());
}

#[test]
fn saving_the_defaults_over_no_file_creates_no_file() {
    let (folders, _root) = temp_folders("save-defaults");

    SettingsFiles::load(folders.clone())
        .write(&Settings::default())
        .unwrap();
    let created = folders.settings_file().exists();

    assert!(!created);
}

#[test]
fn saving_creates_the_folder_it_needs() {
    let (folders, _root) = temp_folders("save-folder");
    fs::remove_dir(&folders.settings).unwrap();

    SettingsFiles::load(folders.clone())
        .write(&hidden())
        .unwrap();
    let created = folders.settings_file().exists();

    assert!(created);
}

#[test]
fn saving_keeps_comments_order_and_values_it_doesnt_know() {
    let before = "\
# My settings.

zebra = 1 # From a newer Tsugiki.

# The bar.
[status_bar]
another = 2

# Later.
[later]
alpha = \"x\"
";
    let after = saved_over("save-keeps", before, &hidden());

    assert_eq!(
        after,
        "\
# My settings.

zebra = 1 # From a newer Tsugiki.

# The bar.
[status_bar]
another = 2
visible = false

# Later.
[later]
alpha = \"x\"
"
    );
}

#[test]
fn changing_a_value_keeps_the_comment_beside_it() {
    let before = "[status_bar]\nvisible = true # For now.\n";
    let after = saved_over("save-beside", before, &hidden());
    assert_eq!(after, "[status_bar]\nvisible = false # For now.\n");
}

#[test]
fn a_value_back_at_its_default_goes_with_a_section_left_empty() {
    let before = "first = 1\n\n[status_bar]\nvisible = false\n";
    let after = saved_over("save-removes", before, &Settings::default());
    assert_eq!(after, "first = 1\n");
}

#[test]
fn a_value_back_at_its_default_stays_if_the_user_commented_on_it() {
    let above = "[status_bar]\n# Hidden for screenshots.\nvisible = false\n";
    assert_eq!(
        saved_over("save-above", above, &Settings::default()),
        "[status_bar]\n# Hidden for screenshots.\nvisible = true\n"
    );
    let beside = "[status_bar]\nvisible = false # Screenshots.\n";
    assert_eq!(
        saved_over("save-beside-default", beside, &Settings::default()),
        "[status_bar]\nvisible = true # Screenshots.\n"
    );
}

#[test]
fn a_section_written_inline_or_dotted_is_changed_where_it_is() {
    assert_eq!(
        saved_over(
            "save-inline",
            "status_bar = { visible = true }\n",
            &hidden()
        ),
        "status_bar = { visible = false }\n"
    );
    assert_eq!(
        saved_over("save-dotted", "status_bar.visible = true\n", &hidden()),
        "status_bar.visible = false\n"
    );
}

#[test]
fn saving_keeps_an_edit_made_since_the_last_read() {
    let (folders, _root) = temp_folders("save-edit");
    let mut files = SettingsFiles::load(folders.clone());

    fs::write(folders.settings_file(), "# Added by hand.\nmine = 1\n").unwrap();
    files.write(&hidden()).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(
        after,
        "# Added by hand.\nmine = 1\n\n[status_bar]\nvisible = false\n"
    );
}

#[test]
fn saving_follows_settings_path() {
    let (folders, root) = temp_folders("save-moved");
    let moved = root.join("dotfiles.toml");
    point_at(&folders, &moved);
    let mut files = SettingsFiles::load(folders.clone());

    files.write(&hidden()).unwrap();
    let after = fs::read_to_string(&moved).unwrap();
    let usual_folder_used = folders.settings_file().exists();
    let loaded = files.loaded().clone();

    assert_eq!(after, "[status_bar]\nvisible = false\n");
    assert!(!usual_folder_used);
    assert_eq!(loaded.settings, hidden());
    assert!(loaded.problems.is_empty(), "the file is there now");
}

#[test]
fn saving_over_a_file_that_doesnt_parse_refuses_and_leaves_it_alone() {
    for (name, text) in [
        ("save-broken", "\n[status_bar\n"),
        ("save-wrong-type", "\n[status_bar]\nvisible = \"no\"\n"),
    ] {
        let (folders, _root) = temp_folders(name);
        fs::write(folders.settings_file(), text).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.write(&hidden()).unwrap_err();
        let after = fs::read_to_string(folders.settings_file()).unwrap();

        assert_eq!(after, text);
        assert!(
            refused.message.contains("settings.toml, line"),
            "{}",
            refused.message
        );
        assert_eq!(files.loaded().settings, Settings::default());
    }
}

#[test]
fn a_broken_machine_toml_refuses_rather_than_saving_to_the_wrong_place() {
    let (folders, _root) = temp_folders("save-machine");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.write(&hidden()).unwrap_err();
    let created = folders.settings_file().exists();

    assert!(!created);
    assert_eq!(refused.kind, ErrorKind::Internal);
    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
}

#[test]
fn a_settings_file_it_cant_read_refuses() {
    let (folders, _root) = temp_folders("save-unreadable");
    fs::create_dir(folders.settings_file()).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.write(&hidden()).unwrap_err();

    assert!(
        refused.message.contains("settings.toml"),
        "{}",
        refused.message
    );
}

#[test]
#[expect(
    clippy::permissions_set_readonly_false,
    reason = "the test's own file, made writable again only so it can be removed"
)]
fn a_save_that_fails_says_so_and_leaves_the_file_alone() {
    let (folders, _root) = temp_folders("save-fails");
    let text = "mine = 1\n";
    let file = folders.settings_file();
    fs::write(&file, text).unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let mut files = SettingsFiles::load(folders);

    let refused = files.write(&hidden()).unwrap_err();
    let after = fs::read_to_string(&file).unwrap();
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();

    assert_eq!(after, text);
    assert!(
        refused.message.contains("settings.toml"),
        "{}",
        refused.message
    );
    assert_eq!(files.loaded().settings, Settings::default());
}

#[test]
fn a_comment_on_a_section_keeps_it_when_it_empties() {
    let cases = [
        (
            "save-section",
            "# The bar.\n[status_bar]\nvisible = false\n",
            "# The bar.\n[status_bar]\n",
        ),
        (
            "save-header",
            "[status_bar] # Note.\nvisible = false\n",
            "[status_bar] # Note.\n",
        ),
        (
            "save-inline-beside",
            "status_bar = { visible = false } # Note.\n",
            "status_bar = {} # Note.\n",
        ),
        (
            "save-inline-above",
            "# Note.\nstatus_bar = { visible = false }\n",
            "# Note.\nstatus_bar = {}\n",
        ),
    ];
    for (name, before, expected) in cases {
        assert_eq!(saved_over(name, before, &Settings::default()), expected);
    }
}

#[test]
fn saving_keeps_a_files_line_endings_and_byte_order_mark() {
    assert_eq!(
        saved_over("save-crlf", "# Mine.\r\nmine = 1\r\n", &hidden()),
        "# Mine.\r\nmine = 1\r\n\r\n[status_bar]\r\nvisible = false\r\n"
    );
    assert_eq!(
        saved_over("save-bom", "\u{feff}mine = 1\n", &hidden()),
        "\u{feff}mine = 1\n\n[status_bar]\nvisible = false\n"
    );
}

#[test]
fn saving_what_a_file_already_says_changes_nothing() {
    let before = "\u{feff}# Mine.\r\n[status_bar]\r\nvisible = false\r\n";
    assert_eq!(saved_over("save-same", before, &hidden()), before);
}

#[test]
fn a_settings_path_starting_with_a_tilde_is_a_problem_not_a_folder() {
    let (folders, _root) = temp_folders("tilde");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    let machine = "settings_path = \"~/dotfiles/settings.toml\"\n";
    fs::write(folders.machine_file(), machine).unwrap();

    let mut files = SettingsFiles::load(folders.clone());
    let refused = files.write(&hidden()).unwrap_err();
    let tilde_folder = folders.data.join("~").exists();

    assert_eq!(files.loaded().settings, Settings::default());
    let problem = only_problem(&files);
    assert!(problem.contains("machine.toml"), "{problem}");
    assert!(problem.contains('~'), "{problem}");
    assert!(refused.message.contains('~'), "{}", refused.message);
    assert!(!tilde_folder);
}

#[test]
fn a_machine_toml_broken_from_the_start_doesnt_read_a_settings_file_it_might_not_mean() {
    let (folders, _root) = temp_folders("machine-broken-start");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();

    let files = SettingsFiles::load(folders);

    assert_eq!(files.loaded().settings, Settings::default());
    assert_eq!(files.loaded().problems.len(), 1);
}

#[test]
fn the_owner_saves_an_addition_and_answers_with_what_the_files_say() {
    let (folders, _root) = temp_folders("owner");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

    let saved = owner
        .add_repository(repository("python/cpython"), MirrorSource::Download)
        .unwrap();
    let text = fs::read_to_string(folders.settings_file()).unwrap();

    assert!(text.contains("python/cpython"), "{text}");
    assert_eq!(
        saved.home,
        opened(&["python/cpython"], Conventions::Cpython)
    );
    assert_eq!(saved.loaded.settings.repositories.len(), 1);
    // In the view by the time the owner answers.
    assert_eq!(owner.view(), saved);
}

#[test]
fn the_owner_passes_on_why_a_file_refused_an_addition() {
    let (folders, _root) = temp_folders("owner-refused");
    fs::write(folders.settings_file(), "[status_bar\n").unwrap();
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    let refused = owner
        .add_repository(repository("a/b"), MirrorSource::Download)
        .unwrap_err();

    assert!(
        refused.message.contains("settings.toml"),
        "{}",
        refused.message
    );
}

#[test]
fn without_folders_the_owner_says_what_went_wrong_with_them() {
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
    let owner = SettingsOwner::start(Err(lost.clone()));

    let adding = owner.add_repository(repository("a/b"), MirrorSource::Download);
    let switching = owner.put_on_workbench(repository("a/b"), query("label:bug"), OnWorkbench::On);
    let answering = owner.save_mirror_source(repository("a/b"), MirrorSource::Download);
    let resizing = owner.save_panes(WIDE);
    let running = owner.remember_command(CommandId::new("Reset Layout"));
    let opening = owner.open_repository(repository("a/b"));
    let picking = owner.save_theme(ThemeChoice::System);
    let rereading = owner.reread();
    let applying = owner.apply(SettingsChanges::default());

    assert_eq!(picking.unwrap_err(), lost);
    assert_eq!(rereading.unwrap_err(), lost);
    assert_eq!(applying.unwrap_err(), lost);
    assert_eq!(adding.unwrap_err(), lost);
    assert_eq!(switching.unwrap_err(), lost);
    assert_eq!(opening.unwrap_err(), lost);
    assert_eq!(answering.unwrap_err(), lost);
    assert_eq!(resizing.unwrap_err(), lost);
    assert_eq!(running.unwrap_err(), lost);
}

#[test]
fn the_owner_answers_with_what_the_files_said_when_it_started() {
    let (folders, _root) = temp_folders("owner-started");
    fs::write(folders.settings_file(), "[status_bar\n").unwrap();

    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    assert_eq!(owner.view().loaded.problems.len(), 1);
    assert_eq!(owner.view().home, Home::default());
}

#[test]
fn without_folders_the_owner_answers_with_the_defaults() {
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

    let owner = SettingsOwner::start(Err(lost));

    assert_eq!(owner.view(), SettingsView::default());
}

#[test]
fn additions_that_overlap_all_reach_the_view() {
    let (folders, _root) = temp_folders("owner-overlap");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    thread::scope(|scope| {
        for name in ["a/one", "a/two", "a/three", "a/four"] {
            let owner = owner.clone();
            #[expect(
                unused_results,
                reason = "the scope joins each thread, and a panic in one fails the test"
            )]
            scope.spawn(move || {
                owner
                    .add_repository(repository(name), MirrorSource::Download)
                    .unwrap()
            });
        }
    });

    assert_eq!(owner.view().loaded.settings.repositories.len(), 4);
}

#[test]
fn an_owner_whose_thread_has_gone_says_so() {
    let owner = SettingsOwner {
        requests: mpsc::channel().0,
        view: latest::channel(SettingsView::default()).1,
        commands: latest::channel(Vec::new()).1,
    };

    let refused = owner
        .add_repository(repository("a/b"), MirrorSource::Download)
        .unwrap_err();

    assert_eq!(refused.message, STOPPED_SAVING);
}

/// Longer than a working owner takes to save.
const PLENTY: Duration = Duration::from_secs(60);

/// The owner is held up in the middle of adding a Repository, with a pane layout to save asked after it, as
/// when the user drags a split and closes the window at once (#939).
#[test]
fn stop_waits_for_the_save_under_way_and_the_saves_asked_before_it() {
    let (folders, _root) = temp_folders("owner-stop");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    let (arrived, arrival) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (reply, panes_answer) = mpsc::channel();
    let mut panes = Some(Request::from(Change::Panes {
        layout: WIDE,
        reply,
    }));
    let requests = owner.requests.clone();
    owner.tell_session(Box::new(move |_| {
        // From the owner's own thread, so the pane layout waits behind the addition this holds up.
        if let Some(panes) = panes.take() {
            requests.send(panes).unwrap();
            arrived.send(()).unwrap();
            released.recv_timeout(PLENTY).unwrap();
        }
    }));
    let adding = {
        let owner = owner.clone();
        thread::spawn(move || owner.add_repository(repository("a/b"), MirrorSource::Download))
    };
    arrival.recv_timeout(PLENTY).unwrap();
    // Late enough that a `stop` that didn't wait would have returned before it.
    let releasing = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        release.send(()).unwrap();
    });

    owner.begin_stopping().wait_for_end(PLENTY);

    let after_stop = owner.save_panes(PaneLayout {
        sidebar: Some(300),
        ..WIDE
    });
    releasing.join().unwrap();
    assert!(adding.join().unwrap().is_ok());
    assert!(panes_answer.try_recv().unwrap().is_ok());
    assert_eq!(SettingsFiles::load(folders).view().panes, WIDE);
    assert_eq!(after_stop.unwrap_err().message, STOPPED_SAVING);
}

const WIDE: PaneLayout = PaneLayout {
    sidebar: Some(240),
    list: Some(500),
    peek: None,
    split_peek: None,
    rail: None,
    sidebar_hidden: false,
    sidebar_icons: false,
    list_hidden: false,
    detail_only_hid: None,
    order: ListOrder::Attention,
};

#[test]
fn the_peeks_height_stays_on_this_machine_until_it_goes_back_to_its_default() {
    let (folders, _root) = temp_folders("panes-peek");
    let mut files = SettingsFiles::load(folders.clone());
    let taller = PaneLayout {
        peek: Some(24),
        ..WIDE
    };

    files.save_panes(taller).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let back = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(saved, "[panes]\nsidebar = 240\nlist = 500\npeek = 24\n");
    assert_eq!(read_again, taller);
    assert_eq!(back, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn the_split_peeks_height_stays_on_this_machine_until_it_goes_back_to_its_default() {
    let (folders, _root) = temp_folders("panes-split-peek");
    let mut files = SettingsFiles::load(folders.clone());
    let taller = PaneLayout {
        split_peek: Some(320),
        ..WIDE
    };

    files.save_panes(taller).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let back = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        saved,
        "[panes]\nsidebar = 240\nlist = 500\nsplit_peek = 320\n"
    );
    assert_eq!(read_again, taller);
    assert_eq!(back, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn the_rails_width_stays_on_this_machine_until_it_goes_back_to_its_default() {
    let (folders, _root) = temp_folders("panes-rail");
    let mut files = SettingsFiles::load(folders.clone());
    let wider = PaneLayout {
        rail: Some(360),
        ..WIDE
    };

    files.save_panes(wider).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let back = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(saved, "[panes]\nsidebar = 240\nlist = 500\nrail = 360\n");
    assert_eq!(read_again, wider);
    assert_eq!(back, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn machine_toml_names_each_order_and_pane_as_serde_reads_them() {
    for order in [ListOrder::Attention, ListOrder::LastUpdate] {
        assert_eq!(serde_json::to_value(order).unwrap(), order.to_text());
    }
    for pane in [Panes::Sidebar, Panes::List, Panes::Both] {
        assert_eq!(serde_json::to_value(pane).unwrap(), pane.to_text());
    }
}

#[test]
fn the_lists_order_stays_on_this_machine_until_it_goes_back_to_attention() {
    let (folders, _root) = temp_folders("panes-order");
    let mut files = SettingsFiles::load(folders.clone());
    let by_update = PaneLayout {
        order: ListOrder::LastUpdate,
        ..WIDE
    };

    files.save_panes(by_update).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let back = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        saved,
        "[panes]\nsidebar = 240\nlist = 500\norder = \"last_update\"\n"
    );
    assert_eq!(read_again, by_update);
    assert_eq!(back, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn a_hidden_sidebar_stays_hidden_on_this_machine_until_it_shows_again() {
    let (folders, _root) = temp_folders("panes-sidebar-hidden");
    let mine = "editor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let hidden = PaneLayout {
        sidebar_hidden: true,
        ..WIDE
    };

    files.save_panes(hidden).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let shown = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        saved,
        format!("{mine}\n[panes]\nsidebar = 240\nlist = 500\nsidebar_hidden = true\n")
    );
    assert_eq!(read_again, hidden);
    assert_eq!(
        shown,
        format!("{mine}\n[panes]\nsidebar = 240\nlist = 500\n")
    );
}

#[test]
fn a_hidden_list_stays_hidden_on_this_machine_beside_the_sidebar() {
    let (folders, _root) = temp_folders("panes-list-hidden");
    let mut files = SettingsFiles::load(folders.clone());
    let hidden = PaneLayout {
        list_hidden: true,
        ..WIDE
    };

    files.save_panes(hidden).unwrap();
    let saved = fs::read_to_string(folders.machine_file()).unwrap();
    let read_again = SettingsFiles::load(folders.clone()).view().panes;
    files.save_panes(WIDE).unwrap();
    let shown = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        saved,
        "[panes]\nsidebar = 240\nlist = 500\nlist_hidden = true\n"
    );
    assert_eq!(read_again, hidden);
    assert_eq!(shown, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn a_sidebar_of_icons_stays_so_on_this_machine_while_hidden_and_until_it_shows_in_full() {
    let (folders, _root) = temp_folders("panes-sidebar-icons");
    let mut files = SettingsFiles::load(folders.clone());
    let icons = PaneLayout {
        sidebar_icons: true,
        ..WIDE
    };
    let hidden = PaneLayout {
        sidebar_hidden: true,
        ..icons
    };

    let mut saved = Vec::new();
    let mut read_again = Vec::new();
    for layout in [icons, hidden] {
        files.save_panes(layout).unwrap();
        saved.push(fs::read_to_string(folders.machine_file()).unwrap());
        read_again.push(SettingsFiles::load(folders.clone()).view().panes);
    }
    files.save_panes(WIDE).unwrap();
    let full = fs::read_to_string(folders.machine_file()).unwrap();

    let widths = "[panes]\nsidebar = 240\nlist = 500\n";
    assert_eq!(
        saved,
        [
            format!("{widths}sidebar_icons = true\n"),
            // A key the file holds stays where it is, and a new one goes at the end.
            format!("{widths}sidebar_icons = true\nsidebar_hidden = true\n"),
        ]
    );
    assert_eq!(read_again, [icons, hidden]);
    assert_eq!(full, widths);
}

#[test]
fn what_ctrl_k_z_hid_stays_on_this_machine_until_it_forgets() {
    let (folders, _root) = temp_folders("panes-detail-only");
    let mut files = SettingsFiles::load(folders.clone());
    let detail_only = |hid| PaneLayout {
        sidebar_hidden: true,
        list_hidden: true,
        detail_only_hid: Some(hid),
        ..WIDE
    };

    let mut saved = Vec::new();
    let mut read_again = Vec::new();
    for hid in [Panes::Sidebar, Panes::List, Panes::Both] {
        files.save_panes(detail_only(hid)).unwrap();
        saved.push(fs::read_to_string(folders.machine_file()).unwrap());
        read_again.push(SettingsFiles::load(folders.clone()).view().panes);
    }
    files.save_panes(WIDE).unwrap();
    let forgotten = fs::read_to_string(folders.machine_file()).unwrap();

    let both_hidden =
        "[panes]\nsidebar = 240\nlist = 500\nsidebar_hidden = true\nlist_hidden = true\n";
    assert_eq!(
        saved,
        ["sidebar", "list", "both"]
            .map(|hid| format!("{both_hidden}detail_only_hid = \"{hid}\"\n"))
    );
    assert_eq!(
        read_again,
        [Panes::Sidebar, Panes::List, Panes::Both].map(detail_only)
    );
    assert_eq!(forgotten, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn a_shown_sidebar_keeps_the_users_comment_on_it() {
    let (folders, _root) = temp_folders("panes-sidebar-comment");
    let mine = "[panes]\nsidebar_hidden = true # Small screen.\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(PaneLayout::default()).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(machine, "[panes]\nsidebar_hidden = false # Small screen.\n");
}

#[test]
fn the_panes_layout_stays_on_this_machine_beside_what_the_user_wrote() {
    let (folders, _root) = temp_folders("panes");
    let mine = "# This laptop.\neditor = \"code\" # Mine.\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(WIDE).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let synced = folders.settings_file().exists();
    let read_again = SettingsFiles::load(folders).view().panes;

    assert_eq!(
        machine,
        format!("{mine}\n[panes]\nsidebar = 240\nlist = 500\n")
    );
    assert_eq!(
        (synced, files.view().panes, read_again),
        (false, WIDE, WIDE)
    );
}

#[test]
fn a_pane_back_at_its_default_leaves_machine_toml() {
    let (folders, _root) = temp_folders("panes-default");
    let mine = "editor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    files.save_panes(WIDE).unwrap();

    let sidebar = PaneLayout { list: None, ..WIDE };
    files.save_panes(sidebar).unwrap();
    let one = fs::read_to_string(folders.machine_file()).unwrap();
    files.save_panes(PaneLayout::default()).unwrap();
    let none = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(one, format!("{mine}\n[panes]\nsidebar = 240\n"));
    assert_eq!(
        (none.as_str(), files.view().panes),
        (mine, PaneLayout::default())
    );
}

#[test]
fn a_pane_value_it_cant_read_takes_the_default_and_keeps_the_rest_of_machine_toml() {
    let (folders, _root) = temp_folders("panes-unreadable");
    let text = "last_repository = \"a/b\"

[panes]
sidebar = -1
list = 500
peek = 2.5
sidebar_hidden = \"yes\"
sidebar_icons = \"no\"
list_hidden = 1
detail_only_hid = \"everything\"
order = \"by_mood\"
";
    fs::write(folders.machine_file(), text).unwrap();

    let files = SettingsFiles::load(folders);

    let list = PaneLayout {
        sidebar: None,
        list: Some(500),
        peek: None,
        split_peek: None,
        rail: None,
        sidebar_hidden: false,
        sidebar_icons: false,
        list_hidden: false,
        detail_only_hid: None,
        order: ListOrder::Attention,
    };
    assert_eq!(
        (files.view().panes, files.loaded().problems.len()),
        (list, 0)
    );
}

#[test]
fn a_broken_machine_toml_keeps_the_panes_layout() {
    let (folders, _root) = temp_folders("panes-broken");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.save_panes(WIDE).unwrap_err();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(machine, "settings_path =\n");
    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
}

#[test]
fn the_owner_saves_the_panes_layout_and_shows_it_in_the_view() {
    let (folders, _root) = temp_folders("owner-panes");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    let saved = owner.save_panes(WIDE).unwrap();

    assert_eq!((saved.panes, owner.view().panes), (WIDE, WIDE));
}

#[test]
fn machine_toml_says_where_ty_is_from_its_own_folder() {
    let ty = |text: &str| {
        let (folders, _root) = temp_folders("language-servers");
        fs::write(folders.machine_file(), text).unwrap();
        let files = SettingsFiles::load(folders.clone());
        assert_eq!(files.loaded().problems, Vec::<String>::new());
        let ty = files.view().language_servers.ty;
        ty.map(|path| path.strip_prefix(&folders.data).unwrap().to_path_buf())
    };

    assert_eq!(
        ty("[language_servers.ty]\npath = 'tools/ty.exe'\n"),
        Some(PathBuf::from("tools/ty.exe"))
    );
    assert_eq!(ty("[panes]\nsidebar = 240\n"), None);
    assert_eq!(ty("[language_servers.ty]\nwhere = 'ty.exe'\n"), None);
    assert_eq!(ty("[language_servers]\nty = 3\n"), None);
    assert_eq!(ty("language_servers = 'ty'\n"), None);
}

#[test]
fn machine_toml_says_where_rust_analyzer_is_beside_ty() {
    let (folders, _root) = temp_folders("language-servers-rust");
    let text = "[language_servers.ty]\npath = 'ty.exe'\n[language_servers.rust-analyzer]\npath = 'ra/rust-analyzer.exe'\n";
    fs::write(folders.machine_file(), text).unwrap();

    let servers = SettingsFiles::load(folders.clone()).view().language_servers;

    let expected = LanguageServers {
        ty: Some(folders.data.join("ty.exe")),
        rust_analyzer: Some(folders.data.join("ra/rust-analyzer.exe")),
        clangd: None,
        ..LanguageServers::default()
    };
    assert_eq!(servers, expected);
}

#[test]
fn machine_toml_says_where_clangd_is() {
    let (folders, _root) = temp_folders("language-servers-clangd");
    let text = "[language_servers.clangd]\npath = 'clangd/bin/clangd.exe'\n";
    fs::write(folders.machine_file(), text).unwrap();

    let servers = SettingsFiles::load(folders.clone()).view().language_servers;

    let expected = LanguageServers {
        ty: None,
        rust_analyzer: None,
        clangd: Some(folders.data.join("clangd/bin/clangd.exe")),
        ..LanguageServers::default()
    };
    assert_eq!(servers, expected);
}

#[test]
fn machine_toml_says_how_long_an_idle_server_runs_and_30_minutes_stands_for_what_it_cant_read() {
    let idle = |text: &str| {
        let (folders, _root) = temp_folders("language-servers-idle");
        fs::write(folders.machine_file(), text).unwrap();
        let files = SettingsFiles::load(folders);
        assert_eq!(files.loaded().problems, Vec::<String>::new());
        files.view().language_servers.idle.as_secs() / 60
    };

    assert_eq!(idle("[language_servers]\nidle_minutes = 5\n"), 5);
    let with_a_server =
        "[language_servers]\nidle_minutes = 90\n[language_servers.ty]\npath = 'ty.exe'\n";
    assert_eq!(idle(with_a_server), 90);
    assert_eq!(idle(""), 30);
    for unread in ["0", "-1", "'ten'", "2.5"] {
        assert_eq!(
            idle(&format!("[language_servers]\nidle_minutes = {unread}\n")),
            30
        );
    }
}

fn minutes(count: u32) -> NonZeroU32 {
    NonZeroU32::new(count).unwrap()
}

#[test]
fn settings_advanced_shows_each_server_s_path_as_machine_toml_writes_it() {
    let (folders, _root) = temp_folders("language-servers-loaded");
    let text = "[language_servers]\nidle_minutes = 5\n[language_servers.clangd]\npath = 'clangd/bin/clangd.exe'\n";
    fs::write(folders.machine_file(), text).unwrap();

    let loaded = SettingsFiles::load(folders)
        .loaded()
        .language_servers
        .clone();

    let expected = LanguageServerSettings {
        clangd: Some(PathBuf::from("clangd/bin/clangd.exe")),
        idle_minutes: minutes(5),
        ..LanguageServerSettings::default()
    };
    assert_eq!(loaded, expected);
}

#[test]
fn apply_saves_each_server_s_path_and_the_idle_time_in_machine_toml_and_leaves_settings_toml_alone()
{
    let (folders, _root) = temp_folders("language-servers-apply");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    let servers = LanguageServerSettings {
        ty: Some(PathBuf::from("tools/ty.exe")),
        rust_analyzer: Some(PathBuf::from("ra/rust-analyzer.exe")),
        idle_minutes: minutes(10),
        ..LanguageServerSettings::default()
    };

    let answer = owner.apply(SettingsChanges {
        language_servers: Some(servers.clone()),
        ..SettingsChanges::default()
    });
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let settings_saved = folders.settings_file().exists();
    let restarted = SettingsFiles::load(folders.clone()).view().language_servers;

    assert_eq!(answer.unwrap().loaded.language_servers, servers);
    assert_eq!(
        machine,
        "[language_servers]\nidle_minutes = 10\n\n[language_servers.ty]\npath = \"tools/ty.exe\"\n\n[language_servers.rust-analyzer]\npath = \"ra/rust-analyzer.exe\"\n"
    );
    assert!(!settings_saved);
    assert_eq!(restarted.ty, Some(folders.data.join("tools/ty.exe")));
    assert_eq!(restarted.idle, Duration::from_secs(600));
}

#[test]
fn a_server_cleared_in_settings_loses_its_path_and_the_default_idle_time_goes() {
    let (folders, _root) = temp_folders("language-servers-clear");
    let text = "# Mine.\npanes.sidebar = 200\n\n[language_servers]\nidle_minutes = 10\n\n[language_servers.ty]\npath = 'ty.exe'\n\n[language_servers.clangd]\n# Where I unpacked it.\npath = 'clangd.exe'\n";
    fs::write(folders.machine_file(), text).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let applied = files.apply(SettingsChanges {
        language_servers: Some(LanguageServerSettings {
            clangd: Some(PathBuf::from("clangd.exe")),
            ..LanguageServerSettings::default()
        }),
        ..SettingsChanges::default()
    });
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    applied.unwrap();
    assert_eq!(
        machine,
        "# Mine.\npanes.sidebar = 200\n\n[language_servers]\n\n[language_servers.clangd]\n# Where I unpacked it.\npath = 'clangd.exe'\n"
    );
    assert_eq!(files.view().language_servers.ty, None);
}

#[test]
fn apply_over_a_machine_toml_that_stopped_parsing_refuses_and_saves_neither_file() {
    let (folders, _root) = temp_folders("language-servers-broken");
    let mut files = SettingsFiles::load(folders.clone());
    fs::write(folders.machine_file(), "[language_servers\n").unwrap();

    let applied = files.apply(SettingsChanges {
        status_bar: Some(StatusBarSettings { visible: false }),
        language_servers: Some(LanguageServerSettings::default()),
        ..SettingsChanges::default()
    });
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let settings_saved = folders.settings_file().exists();

    assert!(applied.is_err());
    assert_eq!(machine, "[language_servers\n");
    assert!(!settings_saved);
}

#[test]
fn apply_writes_a_server_s_path_over_a_hand_written_path_that_isn_t_a_table() {
    let (folders, _root) = temp_folders("language-servers-not-a-table");
    fs::write(
        folders.machine_file(),
        "[language_servers]\nty = \"tools/ty.exe\"\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let applied = files.apply(SettingsChanges {
        language_servers: Some(LanguageServerSettings {
            ty: Some(PathBuf::from("tools/ty.exe")),
            ..LanguageServerSettings::default()
        }),
        ..SettingsChanges::default()
    });
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    applied.unwrap();
    assert_eq!(
        machine,
        "[language_servers]\n\n[language_servers.ty]\npath = \"tools/ty.exe\"\n"
    );
    // A relative path starts at the data folder, on each OS.
    assert_eq!(
        files.view().language_servers.ty,
        Some(folders.data.join("tools/ty.exe"))
    );
}

#[test]
fn machine_toml_says_where_each_mirror_gets_its_objects_and_skips_what_it_cant_read() {
    let (folders, _root) = temp_folders("mirror-sources");
    let mine = r#"[mirrors]
"python/cpython" = 'D:\cpython'
"octo-org/hello" = "download"
"not a name" = "download"
"octo-org/odd" = 3
"#;
    fs::write(folders.machine_file(), mine).unwrap();

    let files = SettingsFiles::load(folders);
    let sources = files.view().mirror_sources;

    assert_eq!(files.loaded().problems, Vec::<String>::new());
    assert_eq!(
        sources.get(&repository("Python/CPython")),
        Some(&MirrorSource::Borrow {
            folder: PathBuf::from(r"D:\cpython")
        })
    );
    assert_eq!(
        sources.get(&repository("octo-org/hello")),
        Some(&MirrorSource::Download)
    );
    assert_eq!(sources.get(&repository("octo-org/odd")), None);
}

#[test]
fn a_mirrors_answer_saves_under_one_key_beside_what_the_user_wrote() {
    let (folders, _root) = temp_folders("mirror-source-saved");
    let mine = r#"# This laptop.

[mirrors]
"Python/CPython" = "download"
"a/b" = "download" # Mine.
"#;
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let lender = MirrorSource::Borrow {
        folder: PathBuf::from(r"D:\cpython"),
    };

    files
        .save_mirror_source(&repository("python/cpython"), &lender)
        .unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    let wanted = r#"# This laptop.

[mirrors]
"a/b" = "download" # Mine.
"python/cpython" = 'D:\cpython'
"#;
    assert_eq!(machine, wanted);
    assert_eq!(
        files
            .view()
            .mirror_sources
            .get(&repository("python/cpython")),
        Some(&lender)
    );
}

#[test]
fn a_broken_machine_toml_keeps_a_mirrors_answer_out() {
    let (folders, _root) = temp_folders("mirror-source-broken");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files
        .save_mirror_source(&repository("a/b"), &MirrorSource::Download)
        .unwrap_err();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(machine, "settings_path =\n");
    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
}

#[test]
fn the_owner_saves_a_mirrors_answer_and_shows_it_in_the_view() {
    let (folders, _root) = temp_folders("owner-mirror-source");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));
    let cpython = repository("python/cpython");

    let saved = owner
        .save_mirror_source(cpython.clone(), MirrorSource::Download)
        .unwrap();

    assert_eq!(
        saved.mirror_sources.get(&cpython),
        Some(&MirrorSource::Download)
    );
    assert_eq!(owner.view().mirror_sources, saved.mirror_sources);
}

#[test]
fn with_no_repositories_the_window_opens_on_onboarding() {
    let (folders, _root) = temp_folders("home-none");
    let files = SettingsFiles::load(folders);
    assert_eq!(files.home(), Home::default());
}

#[test]
fn adding_a_repository_saves_it_with_its_conventions() {
    let (folders, _root) = temp_folders("add");
    let mut files = SettingsFiles::load(folders.clone());

    let home = files
        .add_repository(
            &RepositorySettings::new(repository("python/cpython")),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let text = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(home, opened(&["python/cpython"], Conventions::Cpython));
    assert_eq!(
        text,
        "[[repositories]]\nname = \"python/cpython\"\nconventions = \"cpython\"\n
[[repositories.filters]]\nname = \"Mine\"\nquery = \"is:open author:@me\"\non_workbench = true\n
[[repositories.filters]]\nname = \"Involved\"\nquery = \"is:open involves:@me -author:@me\"\non_workbench = false\n"
    );
    assert_eq!(files.loaded().settings.repositories.len(), 1);
}

#[test]
fn another_repository_goes_after_the_others_and_keeps_the_users_words() {
    let (folders, _root) = temp_folders("add-second");
    let before = "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
conventions = \"cpython\"

# The bar.
[status_bar]
visible = false
";
    fs::write(folders.settings_file(), before).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let home = files
        .add_repository(
            &RepositorySettings::new(repository("some-maintainer/gcmon")),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(
        home,
        opened(
            &["some-maintainer/gcmon", "python/cpython"],
            Conventions::Github
        )
    );
    assert_eq!(
        after,
        "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
conventions = \"cpython\"

[[repositories]]
name = \"some-maintainer/gcmon\"
conventions = \"github\"

[[repositories.filters]]
name = \"Mine\"
query = \"is:open author:@me\"
on_workbench = true

[[repositories.filters]]
name = \"Involved\"
query = \"is:open involves:@me -author:@me\"
on_workbench = false

# The bar.
[status_bar]
visible = false
"
    );
}

#[test]
fn a_list_written_inline_stays_inline() {
    let (folders, _root) = temp_folders("add-inline");
    fs::write(
        folders.settings_file(),
        "repositories = [{ name = \"a/b\" }]\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    drop(
        files
            .add_repository(
                &RepositorySettings::new(repository("c/d")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let loaded = files.loaded().clone();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    let names: Vec<_> = loaded
        .settings
        .repositories
        .iter()
        .map(|one| one.name.to_string())
        .collect();
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(names, ["a/b", "c/d"]);
    // A read fills in the starters when a Repository has no Filters, so the file has to say them.
    assert_eq!(loaded.settings.repositories[1].filters, Filter::starters());
    assert!(after.starts_with("repositories = [{"), "{after}");
    assert!(after.contains("Involved"), "{after}");
}

#[test]
fn adding_a_repository_twice_keeps_one_and_opens_it() {
    let (folders, _root) = temp_folders("add-twice");
    let mut files = SettingsFiles::load(folders.clone());
    drop(
        files
            .add_repository(
                &RepositorySettings::new(repository("python/cpython")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    drop(
        files
            .add_repository(
                &RepositorySettings::new(repository("a/b")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let before = fs::read_to_string(folders.settings_file()).unwrap();

    let home = files
        .add_repository(
            &RepositorySettings::new(repository("Python/CPython")),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();

    assert_eq!(after, before);
    assert_eq!(
        home,
        opened(&["python/cpython", "a/b"], Conventions::Cpython)
    );
}

#[test]
fn adding_puts_the_repository_first_in_recent_with_the_time() {
    let (folders, _root) = temp_folders("recent");
    let mut files = SettingsFiles::load(folders.clone());

    add_all(
        &mut files,
        &["a/first", "a/second", "A/First"],
        moment("2026-11-10T09:00:00.5+03:00"),
    );
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let settings = fs::read_to_string(folders.settings_file()).unwrap();
    let home = files.home();

    assert_eq!(
        machine,
        "recent = [
    { name = \"A/First\", opened_at = 2026-11-10T08:00:00Z },
    { name = \"a/second\", opened_at = 2026-11-10T07:00:00Z },
]

[mirrors]
\"a/second\" = \"download\"
\"A/First\" = \"download\"
"
    );
    assert!(!settings.contains(RECENT), "{settings}");
    assert_eq!(home, opened(&["a/first", "a/second"], Conventions::Github));
}

#[test]
fn a_normal_launch_reopens_the_head_of_recent() {
    let (folders, _root) = temp_folders("reopen");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);

    let reopened = SettingsFiles::load(folders).home();

    assert_eq!(
        reopened,
        opened(&["a/second", "a/first"], Conventions::Github)
    );
}

#[test]
fn a_launch_skips_a_name_the_settings_no_longer_hold() {
    let (folders, _root) = temp_folders("reopen-skip");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second", "a/gone"], WHENEVER);
    hold_only(&folders, &["a/first", "a/second"]);

    let home = SettingsFiles::load(folders).home();

    assert_eq!(
        home,
        opened_by_hand(&["a/second", "a/first"], Conventions::Github)
    );
}

#[test]
fn the_home_lists_the_repositories_most_recently_opened_first_then_the_others() {
    let (folders, _root) = temp_folders("recent-order");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(
        &mut files,
        &["a/first", "a/second", "a/third", "a/gone"],
        WHENEVER,
    );
    drop(
        files
            .open_repository(&repository("a/first"), WHENEVER)
            .unwrap(),
    );
    hold_only(
        &folders,
        &["a/never", "a/first", "a/second", "a/third", "a/later"],
    );

    let home = SettingsFiles::load(folders).home();

    // The settings no longer hold a/gone, and a/never and a/later were never opened on this machine.
    assert_eq!(
        home,
        opened_by_hand(
            &["a/first", "a/third", "a/second", "a/never", "a/later"],
            Conventions::Github
        )
    );
}

#[test]
fn with_no_name_in_recent_left_a_launch_opens_the_first_repository() {
    let (folders, _root) = temp_folders("reopen-gone");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/gone"], WHENEVER);
    hold_only(&folders, &["a/first", "a/second"]);

    let home = SettingsFiles::load(folders).home();

    assert_eq!(
        home,
        opened_by_hand(&["a/first", "a/second"], Conventions::Github)
    );
}

#[test]
fn an_entry_in_recent_it_cant_read_is_passed_over_and_kept() {
    let (folders, _root) = temp_folders("recent-unreadable");
    hold_only(&folders, &["a/first", "a/second"]);
    let mine = "recent = [
    { name = \"nonsense\" },
    7,
    { name = \"a/second\", opened_at = \"yesterday\" },
]
";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let home = files.home();
    let problems = files.loaded().problems.clone();

    add_all(&mut files, &["a/second"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        (home, problems),
        (
            opened_by_hand(&["a/second", "a/first"], Conventions::Github),
            Vec::<String>::new()
        )
    );
    assert!(
        machine.starts_with(
            "recent = [
    { name = \"a/second\", opened_at = 2026-11-10T09:00:00Z },
    { name = \"nonsense\" },
    7,
]
"
        ),
        "{machine}"
    );
}

#[test]
fn a_last_repository_from_0_2_0_reopens_and_the_next_add_turns_it_into_recent() {
    let (folders, _root) = temp_folders("recent-from-last");
    hold_only(&folders, &["a/first", "a/old"]);
    fs::write(
        folders.machine_file(),
        "# This laptop.\nlast_repository = \"a/old\" # Mine.\neditor = \"code\"\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/new"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        reopened,
        opened_by_hand(&["a/old", "a/first"], Conventions::Github)
    );
    assert_eq!(
        machine,
        "# This laptop.
recent = [
    { name = \"a/new\", opened_at = 2026-11-10T09:00:00Z },
    { name = \"a/old\" },
] # Mine.
editor = \"code\"

[mirrors]
\"a/new\" = \"download\"
"
    );
}

#[test]
fn adding_the_last_repository_from_0_2_0_again_leaves_one_entry_for_it() {
    let (folders, _root) = temp_folders("recent-from-last-again");
    fs::write(folders.machine_file(), "last_repository = \"a/old\"\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    add_all(&mut files, &["a/Old"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert!(
        machine.starts_with(
            "recent = [\n    { name = \"a/Old\", opened_at = 2026-11-10T09:00:00Z },\n]\n"
        ),
        "{machine}"
    );
}

#[test]
fn any_write_of_machine_toml_turns_a_last_repository_from_0_2_0_into_recent() {
    let (folders, _root) = temp_folders("recent-from-last-panes");
    hold_only(&folders, &["a/first", "a/old"]);
    let mine = "# This laptop.\nlast_repository = \"a/old\" # Mine.\neditor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(WIDE).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let reopened = SettingsFiles::load(folders).home();

    assert_eq!(
        machine,
        "# This laptop.
recent = [
    { name = \"a/old\" },
] # Mine.
editor = \"code\"

[panes]
sidebar = 240
list = 500
"
    );
    assert_eq!(
        reopened,
        opened_by_hand(&["a/old", "a/first"], Conventions::Github)
    );
}

#[test]
fn a_last_repository_written_by_0_2_0_after_recent_gives_way_to_the_list() {
    let (folders, _root) = temp_folders("recent-and-last");
    hold_only(&folders, &["a/one", "a/old"]);
    let mine = "recent = [\n    { name = \"a/one\" },\n]\nlast_repository = \"a/old\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    files.save_panes(PaneLayout::default()).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        reopened,
        opened_by_hand(&["a/one", "a/old"], Conventions::Github)
    );
    assert_eq!(machine, "recent = [\n    { name = \"a/one\" },\n]\n");
}

#[test]
fn a_recent_that_isnt_a_list_gives_way_to_a_last_repository() {
    let (folders, _root) = temp_folders("recent-not-a-list");
    hold_only(&folders, &["a/first", "a/old"]);
    let mine = "recent = \"a/first\" # Wrong.\nlast_repository = \"a/old\"\neditor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/new"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        reopened,
        opened_by_hand(&["a/old", "a/first"], Conventions::Github)
    );
    assert!(
        machine.starts_with(
            "recent = [
    { name = \"a/new\", opened_at = 2026-11-10T09:00:00Z },
    { name = \"a/old\" },
]
editor = \"code\"
"
        ),
        "{machine}"
    );
}

#[test]
fn a_recent_written_as_tables_keeps_its_entries() {
    let (folders, _root) = temp_folders("recent-tables");
    hold_only(&folders, &["a/one", "a/two"]);
    let mine = "[[recent]]\nname = \"a/one\"\n\n[[recent]]\nname = \"a/two\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/three"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        reopened,
        opened_by_hand(&["a/one", "a/two"], Conventions::Github)
    );
    assert!(
        machine.starts_with(
            "recent = [
    { name = \"a/three\", opened_at = 2026-11-10T09:00:00Z },
    { name = \"a/one\" },
    { name = \"a/two\" },
]
"
        ),
        "{machine}"
    );
}

#[test]
fn a_repository_added_again_keeps_the_comment_on_its_entry() {
    let (folders, _root) = temp_folders("recent-comment");
    let mine = "recent = [
    { name = \"a/one\" },
    # Work.
    { name = \"a/two\" },
]
";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    add_all(&mut files, &["a/two"], moment("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert!(
        machine.starts_with(
            "recent = [
    # Work.
    { name = \"a/two\", opened_at = 2026-11-10T09:00:00Z },
    { name = \"a/one\" },
]
"
        ),
        "{machine}"
    );
}

#[test]
fn adding_follows_settings_path() {
    let (folders, root) = temp_folders("add-moved");
    let moved = root.join("dotfiles.toml");
    point_at(&folders, &moved);
    let mut files = SettingsFiles::load(folders.clone());

    drop(
        files
            .add_repository(
                &RepositorySettings::new(repository("a/b")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let text = fs::read_to_string(&moved).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert!(text.contains("a/b"), "{text}");
    assert!(machine.contains("settings_path"), "{machine}");
    assert!(machine.contains("{ name = \"a/b\""), "{machine}");
}

#[test]
fn adding_refuses_while_either_file_cant_be_used() {
    for (name, settings, machine, named) in [
        ("add-settings", "[status_bar\n", "", "settings.toml, line 1"),
        (
            "add-machine",
            "",
            "settings_path =\n",
            "machine.toml, line 1",
        ),
    ] {
        let (folders, _root) = temp_folders(name);
        fs::write(folders.settings_file(), settings).unwrap();
        fs::write(folders.machine_file(), machine).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files
            .add_repository(
                &RepositorySettings::new(repository("a/b")),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap_err();
        let after = fs::read_to_string(folders.settings_file()).unwrap();

        assert_eq!(after, settings);
        assert!(refused.message.contains(named), "{}", refused.message);
    }
}

#[test]
#[expect(
    clippy::permissions_set_readonly_false,
    reason = "the test's own file, made writable again only so it can be removed"
)]
fn an_add_that_cant_save_machine_toml_says_so_and_adds_nothing() {
    let (folders, _root) = temp_folders("add-machine-readonly");
    let file = folders.machine_file();
    fs::write(&file, "mine = 1\n").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files
        .add_repository(
            &RepositorySettings::new(repository("a/b")),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap_err();
    let added = folders.settings_file().exists();
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();

    assert!(
        refused.message.contains("machine.toml"),
        "{}",
        refused.message
    );
    assert!(!added);
    assert_eq!(files.home(), Home::default());
}

#[test]
fn opening_a_repository_puts_it_first_in_recent_and_the_next_launch_opens_it() {
    let (folders, _root) = temp_folders("open");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(
        &mut files,
        &["a/first", "a/second"],
        moment("2026-11-10T07:00:00Z"),
    );
    let settings = fs::read_to_string(folders.settings_file()).unwrap();

    let home = files
        .open_repository(
            &repository("A/First"),
            moment("2026-11-10T09:00:00.5+01:00"),
        )
        .unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let relaunched = SettingsFiles::load(folders).home();

    assert_eq!(home, opened(&["a/first", "a/second"], Conventions::Github));
    assert_eq!(relaunched, home);
    // Under the name the settings hold, and with the mirror answers as they were.
    assert_eq!(
        machine,
        "recent = [
    { name = \"a/first\", opened_at = 2026-11-10T08:00:00Z },
    { name = \"a/second\", opened_at = 2026-11-10T08:00:00Z },
]

[mirrors]
\"a/first\" = \"download\"
\"a/second\" = \"download\"
"
    );
    assert_eq!(after, settings);
}

#[test]
fn opening_a_repository_the_settings_dont_hold_is_refused_and_changes_nothing() {
    let (folders, _root) = temp_folders("open-unknown");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    let refused = files
        .open_repository(&repository("a/gone"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(refused.kind, ErrorKind::NotFound);
    assert_eq!(refused.message, "Tsugiki doesn't have a/gone.");
    assert_eq!(after, machine);
    assert_eq!(
        files.home(),
        opened(&["a/second", "a/first"], Conventions::Github)
    );
}

#[test]
fn opening_a_repository_the_user_took_out_of_settings_toml_since_is_refused() {
    let (folders, _root) = temp_folders("open-removed");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    hold_only(&folders, &["a/second"]);
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    let refused = files
        .open_repository(&repository("a/first"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(refused.message, "Tsugiki doesn't have a/first.");
    assert_eq!(after, machine);
}

#[test]
fn opening_refuses_while_machine_toml_cant_be_used() {
    let (folders, _root) = temp_folders("open-machine-broken");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();

    let refused = files
        .open_repository(&repository("a/first"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();

    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
    assert_eq!(after, "settings_path =\n");
}

#[test]
fn the_owner_opens_a_repository_and_answers_with_what_the_files_say() {
    let (folders, _root) = temp_folders("owner-open");
    let mut files = SettingsFiles::load(folders);
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let owner = SettingsOwner::start(Ok(files));

    let saved = owner.open_repository(repository("a/first")).unwrap();

    assert_eq!(
        saved.home,
        opened(&["a/first", "a/second"], Conventions::Github)
    );
    // In the view by the time the owner answers.
    assert_eq!(owner.view(), saved);
}

#[test]
fn the_owners_check_reads_the_files_again_so_a_repository_taken_out_by_hand_isnt_held() {
    let (folders, _root) = temp_folders("owner-check");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let owner = SettingsOwner::start(Ok(files));
    hold_only(&folders, &["a/first"]);

    let second = owner.check_name(repository("a/second"), &mut |_| Ok(found("a/second")));
    let first = owner.check_name(repository("A/First"), &mut |_| {
        panic!("GitHub was asked about a Repository Tsugiki has")
    });

    assert_eq!(
        second.unwrap(),
        NameCheck::New {
            check: found("a/second")
        }
    );
    assert_eq!(
        first.unwrap(),
        NameCheck::Held {
            name: repository("a/first")
        }
    );
    // What the owner read is in the view too, so the picker's list loses the name.
    assert_eq!(
        owner.view().home,
        opened_by_hand(&["a/first"], Conventions::Github)
    );
}

#[test]
fn without_folders_the_owners_check_asks_github() {
    let owner = SettingsOwner::start(Err(Error::new(ErrorKind::Internal, "no folders")));

    let checked = owner.check_name(repository("a/b"), &mut |_| Ok(found("a/b")));

    assert_eq!(
        checked.unwrap(),
        NameCheck::New {
            check: found("a/b")
        }
    );
}

#[test]
fn a_name_that_isnt_one_is_a_problem_named_with_its_line() {
    let (folders, _root) = temp_folders("bad-name");
    fs::write(
        folders.settings_file(),
        "\n[[repositories]]\nname = \"nonsense\"\n",
    )
    .unwrap();
    let files = SettingsFiles::load(folders.clone());
    let in_settings = only_problem(&files);
    fs::write(folders.settings_file(), "").unwrap();
    fs::write(folders.machine_file(), "\nlast_repository = \"nonsense\"\n").unwrap();
    let files = SettingsFiles::load(folders);
    let in_machine = only_problem(&files);

    assert!(
        in_settings.contains("settings.toml, line 3"),
        "{in_settings}"
    );
    assert!(in_machine.contains("machine.toml, line 2"), "{in_machine}");
}

#[test]
fn saving_settings_leaves_the_repositories_and_the_comments_on_them() {
    let before = "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
";
    let (folders, _root) = temp_folders("save-repositories");
    fs::write(folders.settings_file(), before).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.write(&hidden()).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let held = files.loaded().settings.repositories.len();

    assert!(after.starts_with(before), "{after}");
    assert!(after.ends_with("visible = false\n"), "{after}");
    assert_eq!(held, 1);
}

#[test]
fn a_file_gone_from_settings_path_on_a_reload_keeps_the_last_good_values() {
    let (folders, root) = temp_folders("moved-gone");
    let moved = root.join("dotfiles.toml");
    fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
    point_at(&folders, &moved);
    let mut files = SettingsFiles::load(folders);

    fs::remove_file(&moved).unwrap();
    files.reload();

    assert_eq!(files.loaded().settings, hidden());
    let problem = only_problem(&files);
    assert!(problem.contains("dotfiles.toml"), "{problem}");
    assert!(problem.contains("last values"), "{problem}");
}

fn commands(names: &[&str]) -> Vec<CommandId> {
    names.iter().map(|name| CommandId::new(name)).collect()
}

#[test]
fn a_command_run_comes_first_in_recent_commands_and_a_restart_reads_them() {
    let (folders, _root) = temp_folders("commands");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

    let before = owner.recent_commands();
    let answers: Vec<_> = ["Reset Layout", "Pick a Theme", "Reset Layout"]
        .into_iter()
        .map(|name| owner.remember_command(CommandId::new(name)).unwrap())
        .collect();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let restarted = SettingsOwner::start(Ok(SettingsFiles::load(folders))).recent_commands();

    assert_eq!(before, Vec::new());
    assert_eq!(
        machine,
        "recent_commands = [\n    \"Reset Layout\",\n    \"Pick a Theme\",\n]\n"
    );
    assert_eq!(
        answers,
        [
            commands(&["Reset Layout"]),
            commands(&["Pick a Theme", "Reset Layout"]),
            commands(&["Reset Layout", "Pick a Theme"]),
        ]
    );
    assert_eq!(owner.recent_commands(), restarted);
    assert_eq!(restarted, commands(&["Reset Layout", "Pick a Theme"]));
}

#[test]
fn a_command_run_reads_neither_file_again() {
    let (folders, _root) = temp_folders("commands-no-reread");
    let mut files = SettingsFiles::load(folders.clone());
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();

    let saved = files
        .remember_command(&CommandId::new("Reset Layout"))
        .unwrap();

    // The hand edit waits for the next read (ADR 0014).
    assert_eq!(files.loaded().settings, Settings::default());
    assert_eq!(saved, commands(&["Reset Layout"]));
    assert_eq!(files.recent_commands(), saved);
}

#[test]
fn a_command_run_after_a_broken_machine_toml_was_mended_answers_with_the_list_it_saved() {
    let (folders, _root) = temp_folders("commands-mended");
    fs::write(folders.machine_file(), "recent_commands = [\n").unwrap();
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    fs::write(folders.machine_file(), "recent_commands = []\n").unwrap();

    let saved = owner
        .remember_command(CommandId::new("Reset Layout"))
        .unwrap();

    assert_eq!(saved, commands(&["Reset Layout"]));
    assert_eq!(owner.recent_commands(), saved);
}

#[test]
fn a_reread_takes_in_a_hand_edit_to_recent_commands() {
    let (folders, _root) = temp_folders("commands-reread");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    drop(
        owner
            .remember_command(CommandId::new("Reset Layout"))
            .unwrap(),
    );
    fs::write(
        folders.machine_file(),
        "recent_commands = [\"Open Settings\", \"Reset Layout\"]\n",
    )
    .unwrap();

    drop(owner.save_panes(WIDE).unwrap());
    let after_panes = owner.recent_commands();
    fs::write(
        folders.machine_file(),
        "recent_commands = [\"Pick a Theme\"]\n",
    )
    .unwrap();
    drop(owner.reread().unwrap());

    assert_eq!(after_panes, commands(&["Open Settings", "Reset Layout"]));
    assert_eq!(owner.recent_commands(), commands(&["Pick a Theme"]));
}

#[test]
fn recent_commands_skip_what_isnt_text_and_twice_written_ones_and_keep_the_users_comments() {
    let (folders, _root) = temp_folders("commands-hand");
    let mine = "recent_commands = [
    \"Pick a Theme\",
    7,
    # The one I want.
    \"Open Settings\",
    \"Pick a Theme\",
    \"Open Settings\",
]
";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let read = files.recent_commands();
    let problems = files.loaded().problems.clone();

    let saved = files
        .remember_command(&CommandId::new("Open Settings"))
        .unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!(
        (read, problems),
        (commands(&["Pick a Theme", "Open Settings"]), Vec::new())
    );
    assert_eq!(saved, commands(&["Open Settings", "Pick a Theme"]));
    assert_eq!(files.recent_commands(), saved);
    assert_eq!(
        machine,
        "recent_commands = [
    # The one I want.
    \"Open Settings\",
    \"Pick a Theme\",
    7,
]
"
    );
}

#[test]
fn recent_commands_that_arent_a_list_count_as_none_and_a_run_puts_a_list_in_their_place() {
    let (folders, _root) = temp_folders("commands-not-list");
    fs::write(
        folders.machine_file(),
        "# Mine.\nrecent_commands = \"Reset Layout\"\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let read = files.recent_commands();

    let saved = files
        .remember_command(&CommandId::new("Open Settings"))
        .unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    assert_eq!((read, saved), (Vec::new(), commands(&["Open Settings"])));
    assert_eq!(
        machine,
        "# Mine.\nrecent_commands = [\n    \"Open Settings\",\n]\n"
    );
}

#[test]
fn recent_commands_keep_the_50_run_last_and_an_entry_that_isnt_text_doesnt_count() {
    let (folders, _root) = temp_folders("commands-kept");
    fs::write(folders.machine_file(), "recent_commands = [\n    7,\n]\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let mut run = |count: usize| {
        drop(
            files
                .remember_command(&CommandId::new(&format!("Command {count}")))
                .unwrap(),
        );
    };
    for count in 0..COMMANDS_KEPT - 1 {
        run(count);
    }
    let one_short = fs::read_to_string(folders.machine_file()).unwrap();
    run(COMMANDS_KEPT - 1);
    run(COMMANDS_KEPT);
    let kept = files.recent_commands();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let restarted = SettingsFiles::load(folders).recent_commands();

    // The 7 stays while fewer than 50 commands come before it.
    assert!(
        one_short.ends_with("\"Command 0\",\n    7,\n]\n"),
        "{one_short}"
    );
    assert_eq!(kept.len(), COMMANDS_KEPT);
    assert_eq!(kept.first(), Some(&CommandId::new("Command 50")));
    assert_eq!(kept.last(), Some(&CommandId::new("Command 1")));
    assert!(machine.ends_with("\"Command 1\",\n]\n"), "{machine}");
    assert_eq!(restarted, kept);
}

#[test]
fn a_hand_written_list_of_more_than_50_reads_as_its_first_50() {
    let (folders, _root) = temp_folders("commands-long");
    let written: Vec<String> = (0..=COMMANDS_KEPT)
        .map(|count| format!("\"Command {count}\""))
        .collect();
    fs::write(
        folders.machine_file(),
        format!("recent_commands = [{}]\n", written.join(", ")),
    )
    .unwrap();

    let read = SettingsFiles::load(folders).recent_commands();

    assert_eq!(read.len(), COMMANDS_KEPT);
    assert_eq!(read.last(), Some(&CommandId::new("Command 49")));
}
