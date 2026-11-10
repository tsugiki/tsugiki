use super::*;
use crate::conventions::Conventions;
use crate::filters::Filter;
use crate::mirror::MirrorSource;
use crate::repository::{FoundRepository, RepositoryCounts};
use time::format_description::well_known::Rfc3339;

const HIDDEN: Settings = Settings {
    status_bar: StatusBarSettings { visible: false },
    repositories: Vec::new(),
};

/// When a Repository was opened, for a test that doesn't look at the time.
const WHENEVER: OffsetDateTime = OffsetDateTime::UNIX_EPOCH;

fn repository(text: &str) -> RepositoryName {
    RepositoryName::parse(text).unwrap()
}

fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// Adds each of `names` through the settings' own write, the first at `first`, each later one an hour after
/// the one before.
fn add_all(files: &mut SettingsFiles, names: &[&str], first: OffsetDateTime) {
    for (hours, name) in (0..).zip(names) {
        drop(
            files
                .add_repository(
                    &RepositorySettings::added(repository(name), RepositoryAccess::Other),
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
    Home::Repository {
        repository: RepositorySettings {
            name: repository(names[0]),
            conventions,
            filters: Filter::starters(RepositoryAccess::Other),
        },
        others: names[1..].iter().map(|name| repository(name)).collect(),
    }
}

/// Separate data and settings folders, as an installed Tsugiki on Windows has, and the folder around
/// them.
fn temp_folders(name: &str) -> (Folders, PathBuf) {
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
    let (folders, root) = temp_folders(name);
    fs::write(folders.settings_file(), before).unwrap();

    SettingsFiles::load(folders.clone())
        .write(settings)
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();
    after
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
    let (folders, root) = temp_folders("no-files");

    let files = SettingsFiles::load(folders.clone());
    let created = folders.settings_file().exists() || folders.machine_file().exists();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded(), &LoadedSettings::default());
    assert!(files.loaded().settings.status_bar.visible);
    assert!(!created);
}

#[test]
fn a_value_in_settings_toml_is_read_at_start() {
    let (folders, root) = temp_folders("read");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, HIDDEN);
    assert!(files.loaded().problems.is_empty());
}

#[test]
fn values_a_file_leaves_out_or_this_version_doesnt_know_change_nothing() {
    let (folders, root) = temp_folders("unknown");
    let text = "from_a_newer_tsugiki = 1\n[status_bar]\nanother = 2\n";
    fs::write(folders.settings_file(), text).unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded(), &LoadedSettings::default());
}

#[test]
fn a_file_that_doesnt_parse_is_left_alone_and_named_with_its_line() {
    for (name, text) in [
        ("broken", "# Mine.\n[status_bar]\nvisible = = false\n"),
        ("wrong-type", "\n[status_bar]\nvisible = \"no\"\n"),
    ] {
        let (folders, root) = temp_folders(name);
        fs::write(folders.settings_file(), text).unwrap();

        let files = SettingsFiles::load(folders.clone());
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("unreadable");
    fs::create_dir(folders.settings_file()).unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, Settings::default());
    let problem = only_problem(&files);
    assert!(problem.contains("settings.toml"), "{problem}");
}

#[test]
fn a_reload_of_a_broken_file_keeps_the_last_good_values_until_it_is_fixed() {
    let (folders, root) = temp_folders("last-good");
    let file = folders.settings_file();
    fs::write(&file, "[status_bar]\nvisible = false\n").unwrap();
    let mut files = SettingsFiles::load(folders);

    fs::write(&file, "[status_bar\n").unwrap();
    files.reload();
    let while_broken = files.loaded().clone();

    fs::write(&file, "[status_bar]\nvisible = true\n").unwrap();
    files.reload();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(while_broken.settings, HIDDEN);
    assert_eq!(while_broken.problems.len(), 1);
    assert_eq!(files.loaded(), &LoadedSettings::default());
}

#[test]
fn a_settings_toml_removed_from_its_usual_folder_means_the_defaults_again() {
    let (folders, root) = temp_folders("removed");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    fs::remove_file(folders.settings_file()).unwrap();
    files.reload();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded(), &LoadedSettings::default());
}

#[test]
fn an_empty_list_of_filters_stays_in_a_repositorys_table() {
    let mut table = Table::new();
    drop(table.insert(FILTERS, toml_edit::value(toml_edit::Array::new())));

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
    let (folders, root) = temp_folders(name);
    fs::write(folders.settings_file(), text).unwrap();
    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();
    assert_eq!(files.loaded().problems, Vec::<String>::new());
    files.loaded().settings.repositories[0].clone()
}

fn filter(name: &str, query: &str) -> Filter {
    Filter {
        name: name.to_owned().try_into().unwrap(),
        query: query.to_owned().try_into().unwrap(),
    }
}

#[test]
fn adding_a_repository_writes_its_starter_filters_where_the_user_can_edit_them() {
    let (folders, root) = temp_folders("starter-filters");
    let mut files = SettingsFiles::load(folders.clone());

    drop(
        files
            .add_repository(
                &RepositorySettings::added(repository("a/b"), RepositoryAccess::Maintained),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let written = fs::read_to_string(folders.settings_file()).unwrap();
    let read = files.loaded().settings.repositories[0].clone();
    fs::remove_dir_all(root).unwrap();

    assert!(written.contains("[[repositories.filters]]"), "{written}");
    assert!(written.contains(r#"name = "Open Issues""#), "{written}");
    assert!(
        written.contains(r#"query = "is:issue is:open""#),
        "{written}"
    );
    assert_eq!(read.filters, Filter::starters(RepositoryAccess::Maintained));
}

#[test]
fn a_repository_with_no_filters_in_the_file_gets_the_starters() {
    let read = the_repository_in("no-filters", "[[repositories]]\nname = \"a/b\"\n");
    assert_eq!(read.filters, Filter::starters(RepositoryAccess::Other));
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
    let (folders, root) = temp_folders("blank-filter");
    fs::write(
        folders.settings_file(),
        "[[repositories]]\nname = \"a/b\"\n\n[[repositories.filters]]\nname = \"Bugs\"\n\
             query = \" \"\n",
    )
    .unwrap();
    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, Settings::default());
    assert!(files.loaded().problems[0].contains("A Filter's query can't be blank."));
}

#[test]
fn a_repositorys_settings_come_from_the_list_whatever_the_case_of_its_name() {
    let chosen = RepositorySettings {
        name: RepositoryName::parse("python/cpython").unwrap(),
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
        access: RepositoryAccess::Other,
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
    let name = RepositoryName::parse("python/cpython").unwrap();
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
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, HIDDEN);
    assert!(files.loaded().problems.is_empty());
}

#[test]
fn a_relative_settings_path_starts_at_machine_toml() {
    let (folders, root) = temp_folders("relative");
    fs::write(
        folders.data.join("beside.toml"),
        "[status_bar]\nvisible = false\n",
    )
    .unwrap();
    fs::write(folders.machine_file(), "settings_path = \"beside.toml\"\n").unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, HIDDEN);
}

#[test]
fn a_missing_file_at_settings_path_starts_from_the_defaults_and_says_so() {
    let (folders, root) = temp_folders("moved-missing");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    point_at(&folders, &root.join("not-there.toml"));

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

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
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, HIDDEN);
    let problem = only_problem(&files);
    assert!(problem.contains("machine.toml, line 2"), "{problem}");
}

#[test]
fn saving_writes_only_what_differs_from_the_defaults() {
    let (folders, root) = temp_folders("save-new");
    let mut files = SettingsFiles::load(folders.clone());

    files.write(&HIDDEN).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(after, "[status_bar]\nvisible = false\n");
    assert_eq!(files.loaded().settings, HIDDEN);
}

#[test]
fn saving_the_defaults_over_no_file_creates_no_file() {
    let (folders, root) = temp_folders("save-defaults");

    SettingsFiles::load(folders.clone())
        .write(&Settings::default())
        .unwrap();
    let created = folders.settings_file().exists();
    fs::remove_dir_all(root).unwrap();

    assert!(!created);
}

#[test]
fn saving_creates_the_folder_it_needs() {
    let (folders, root) = temp_folders("save-folder");
    fs::remove_dir(&folders.settings).unwrap();

    SettingsFiles::load(folders.clone()).write(&HIDDEN).unwrap();
    let created = folders.settings_file().exists();
    fs::remove_dir_all(root).unwrap();

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
    let after = saved_over("save-keeps", before, &HIDDEN);

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
    let after = saved_over("save-beside", before, &HIDDEN);
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
        saved_over("save-inline", "status_bar = { visible = true }\n", &HIDDEN),
        "status_bar = { visible = false }\n"
    );
    assert_eq!(
        saved_over("save-dotted", "status_bar.visible = true\n", &HIDDEN),
        "status_bar.visible = false\n"
    );
}

#[test]
fn saving_keeps_an_edit_made_since_the_last_read() {
    let (folders, root) = temp_folders("save-edit");
    let mut files = SettingsFiles::load(folders.clone());

    fs::write(folders.settings_file(), "# Added by hand.\nmine = 1\n").unwrap();
    files.write(&HIDDEN).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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

    files.write(&HIDDEN).unwrap();
    let after = fs::read_to_string(&moved).unwrap();
    let usual_folder_used = folders.settings_file().exists();
    let loaded = files.loaded().clone();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(after, "[status_bar]\nvisible = false\n");
    assert!(!usual_folder_used);
    assert_eq!(loaded.settings, HIDDEN);
    assert!(loaded.problems.is_empty(), "the file is there now");
}

#[test]
fn saving_over_a_file_that_doesnt_parse_refuses_and_leaves_it_alone() {
    for (name, text) in [
        ("save-broken", "\n[status_bar\n"),
        ("save-wrong-type", "\n[status_bar]\nvisible = \"no\"\n"),
    ] {
        let (folders, root) = temp_folders(name);
        fs::write(folders.settings_file(), text).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.write(&HIDDEN).unwrap_err();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("save-machine");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.write(&HIDDEN).unwrap_err();
    let created = folders.settings_file().exists();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("save-unreadable");
    fs::create_dir(folders.settings_file()).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.write(&HIDDEN).unwrap_err();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("save-fails");
    let text = "mine = 1\n";
    let file = folders.settings_file();
    fs::write(&file, text).unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let mut files = SettingsFiles::load(folders);

    let refused = files.write(&HIDDEN).unwrap_err();
    let after = fs::read_to_string(&file).unwrap();
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    fs::remove_dir_all(root).unwrap();

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
        saved_over("save-crlf", "# Mine.\r\nmine = 1\r\n", &HIDDEN),
        "# Mine.\r\nmine = 1\r\n\r\n[status_bar]\r\nvisible = false\r\n"
    );
    assert_eq!(
        saved_over("save-bom", "\u{feff}mine = 1\n", &HIDDEN),
        "\u{feff}mine = 1\n\n[status_bar]\nvisible = false\n"
    );
}

#[test]
fn saving_what_a_file_already_says_changes_nothing() {
    let before = "\u{feff}# Mine.\r\n[status_bar]\r\nvisible = false\r\n";
    assert_eq!(saved_over("save-same", before, &HIDDEN), before);
}

#[test]
fn a_settings_path_starting_with_a_tilde_is_a_problem_not_a_folder() {
    let (folders, root) = temp_folders("tilde");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    let machine = "settings_path = \"~/dotfiles/settings.toml\"\n";
    fs::write(folders.machine_file(), machine).unwrap();

    let mut files = SettingsFiles::load(folders.clone());
    let refused = files.write(&HIDDEN).unwrap_err();
    let tilde_folder = folders.data.join("~").exists();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, Settings::default());
    let problem = only_problem(&files);
    assert!(problem.contains("machine.toml"), "{problem}");
    assert!(problem.contains('~'), "{problem}");
    assert!(refused.message.contains('~'), "{}", refused.message);
    assert!(!tilde_folder);
}

#[test]
fn a_machine_toml_broken_from_the_start_doesnt_read_a_settings_file_it_might_not_mean() {
    let (folders, root) = temp_folders("machine-broken-start");
    fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, Settings::default());
    assert_eq!(files.loaded().problems.len(), 1);
}

#[test]
fn the_owner_saves_an_addition_and_answers_with_what_the_files_say() {
    let (folders, root) = temp_folders("owner");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

    let saved = owner
        .add_repository(
            repository("python/cpython"),
            RepositoryAccess::Other,
            MirrorSource::Download,
        )
        .unwrap();
    let text = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("owner-refused");
    fs::write(folders.settings_file(), "[status_bar\n").unwrap();
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    let refused = owner
        .add_repository(
            repository("a/b"),
            RepositoryAccess::Other,
            MirrorSource::Download,
        )
        .unwrap_err();
    fs::remove_dir_all(root).unwrap();

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

    let adding = owner.add_repository(
        repository("a/b"),
        RepositoryAccess::Other,
        MirrorSource::Download,
    );
    let answering = owner.save_mirror_source(repository("a/b"), MirrorSource::Download);
    let resizing = owner.save_panes(WIDE);
    let opening = owner.open_repository(repository("a/b"));

    assert_eq!(adding.unwrap_err(), lost);
    assert_eq!(opening.unwrap_err(), lost);
    assert_eq!(answering.unwrap_err(), lost);
    assert_eq!(resizing.unwrap_err(), lost);
}

#[test]
fn the_owner_answers_with_what_the_files_said_when_it_started() {
    let (folders, root) = temp_folders("owner-started");
    fs::write(folders.settings_file(), "[status_bar\n").unwrap();

    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("owner-overlap");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    thread::scope(|scope| {
        for name in ["a/one", "a/two", "a/three", "a/four"] {
            let owner = owner.clone();
            let _ = scope.spawn(move || {
                owner
                    .add_repository(
                        repository(name),
                        RepositoryAccess::Other,
                        MirrorSource::Download,
                    )
                    .unwrap()
            });
        }
    });
    fs::remove_dir_all(root).unwrap();

    assert_eq!(owner.view().loaded.settings.repositories.len(), 4);
}

#[test]
fn an_owner_whose_thread_has_gone_says_so() {
    let owner = SettingsOwner {
        requests: mpsc::channel().0,
        view: latest::channel(SettingsView::default()).1,
    };

    let refused = owner
        .add_repository(
            repository("a/b"),
            RepositoryAccess::Other,
            MirrorSource::Download,
        )
        .unwrap_err();

    assert_eq!(refused.message, STOPPED_SAVING);
}

const WIDE: PaneLayout = PaneLayout {
    sidebar: Some(240),
    list: Some(500),
    sidebar_hidden: false,
    list_hidden: false,
    detail_only_hid: None,
};

#[test]
fn a_hidden_sidebar_stays_hidden_on_this_machine_until_it_shows_again() {
    let (folders, root) = temp_folders("panes-sidebar-hidden");
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
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("panes-list-hidden");
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
    fs::remove_dir_all(root).unwrap();

    assert_eq!(
        saved,
        "[panes]\nsidebar = 240\nlist = 500\nlist_hidden = true\n"
    );
    assert_eq!(read_again, hidden);
    assert_eq!(shown, "[panes]\nsidebar = 240\nlist = 500\n");
}

#[test]
fn what_ctrl_k_z_hid_stays_on_this_machine_until_it_forgets() {
    let (folders, root) = temp_folders("panes-detail-only");
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
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("panes-sidebar-comment");
    let mine = "[panes]\nsidebar_hidden = true # Small screen.\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(PaneLayout::default()).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(machine, "[panes]\nsidebar_hidden = false # Small screen.\n");
}

#[test]
fn the_panes_layout_stays_on_this_machine_beside_what_the_user_wrote() {
    let (folders, root) = temp_folders("panes");
    let mine = "# This laptop.\neditor = \"code\" # Mine.\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(WIDE).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let synced = folders.settings_file().exists();
    let read_again = SettingsFiles::load(folders).view().panes;
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("panes-default");
    let mine = "editor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    files.save_panes(WIDE).unwrap();

    let sidebar = PaneLayout { list: None, ..WIDE };
    files.save_panes(sidebar).unwrap();
    let one = fs::read_to_string(folders.machine_file()).unwrap();
    files.save_panes(PaneLayout::default()).unwrap();
    let none = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(one, format!("{mine}\n[panes]\nsidebar = 240\n"));
    assert_eq!(
        (none.as_str(), files.view().panes),
        (mine, PaneLayout::default())
    );
}

#[test]
fn a_pane_value_it_cant_read_takes_the_default_and_keeps_the_rest_of_machine_toml() {
    let (folders, root) = temp_folders("panes-unreadable");
    let text = "last_repository = \"a/b\"

[panes]
sidebar = -1
list = 500
sidebar_hidden = \"yes\"
list_hidden = 1
detail_only_hid = \"everything\"
";
    fs::write(folders.machine_file(), text).unwrap();

    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();

    let list = PaneLayout {
        sidebar: None,
        list: Some(500),
        sidebar_hidden: false,
        list_hidden: false,
        detail_only_hid: None,
    };
    assert_eq!(
        (files.view().panes, files.loaded().problems.len()),
        (list, 0)
    );
}

#[test]
fn a_broken_machine_toml_keeps_the_panes_layout() {
    let (folders, root) = temp_folders("panes-broken");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files.save_panes(WIDE).unwrap_err();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(machine, "settings_path =\n");
    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
}

#[test]
fn the_owner_saves_the_panes_layout_and_shows_it_in_the_view() {
    let (folders, root) = temp_folders("owner-panes");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

    let saved = owner.save_panes(WIDE).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!((saved.panes, owner.view().panes), (WIDE, WIDE));
}

#[test]
fn machine_toml_says_where_each_mirror_gets_its_objects_and_skips_what_it_cant_read() {
    let (folders, root) = temp_folders("mirror-sources");
    let mine = r#"[mirrors]
"python/cpython" = 'D:\cpython'
"octo-org/hello" = "download"
"not a name" = "download"
"octo-org/odd" = 3
"#;
    fs::write(folders.machine_file(), mine).unwrap();

    let files = SettingsFiles::load(folders);
    let sources = files.view().mirror_sources;
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("mirror-source-saved");
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
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("mirror-source-broken");
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files
        .save_mirror_source(&repository("a/b"), &MirrorSource::Download)
        .unwrap_err();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(machine, "settings_path =\n");
    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
}

#[test]
fn the_owner_saves_a_mirrors_answer_and_shows_it_in_the_view() {
    let (folders, root) = temp_folders("owner-mirror-source");
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));
    let cpython = repository("python/cpython");

    let saved = owner
        .save_mirror_source(cpython.clone(), MirrorSource::Download)
        .unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(
        saved.mirror_sources.get(&cpython),
        Some(&MirrorSource::Download)
    );
    assert_eq!(owner.view().mirror_sources, saved.mirror_sources);
}

#[test]
fn with_no_repositories_the_window_opens_on_onboarding() {
    let (folders, root) = temp_folders("home-none");
    let files = SettingsFiles::load(folders);
    fs::remove_dir_all(root).unwrap();
    assert_eq!(files.home(), Home::default());
}

#[test]
fn adding_a_repository_saves_it_with_its_conventions() {
    let (folders, root) = temp_folders("add");
    let mut files = SettingsFiles::load(folders.clone());

    let home = files
        .add_repository(
            &RepositorySettings::added(repository("python/cpython"), RepositoryAccess::Other),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let text = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(home, opened(&["python/cpython"], Conventions::Cpython));
    assert_eq!(
        text,
        "[[repositories]]\nname = \"python/cpython\"\nconventions = \"cpython\"\n
[[repositories.filters]]\nname = \"Mine\"\nquery = \"is:open author:@me\"\n
[[repositories.filters]]\nname = \"Involved\"\nquery = \"is:open involves:@me -author:@me\"\n"
    );
    assert_eq!(files.loaded().settings.repositories.len(), 1);
}

#[test]
fn another_repository_goes_after_the_others_and_keeps_the_users_words() {
    let (folders, root) = temp_folders("add-second");
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
            &RepositorySettings::added(
                repository("some-maintainer/gcmon"),
                RepositoryAccess::Other,
            ),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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

[[repositories.filters]]
name = \"Involved\"
query = \"is:open involves:@me -author:@me\"

# The bar.
[status_bar]
visible = false
"
    );
}

#[test]
fn a_list_written_inline_stays_inline() {
    let (folders, root) = temp_folders("add-inline");
    fs::write(
        folders.settings_file(),
        "repositories = [{ name = \"a/b\" }]\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    drop(
        files
            .add_repository(
                &RepositorySettings::added(repository("c/d"), RepositoryAccess::Maintained),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let loaded = files.loaded().clone();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    let names: Vec<_> = loaded
        .settings
        .repositories
        .iter()
        .map(|one| one.name.to_string())
        .collect();
    assert_eq!(loaded.problems, Vec::<String>::new());
    assert_eq!(names, ["a/b", "c/d"]);
    // Maintained, because a read fills in the Other starters when a Repository has no Filters: with
    // Other, an inline table that lost its Filters would read back the same.
    assert_eq!(
        loaded.settings.repositories[1].filters,
        Filter::starters(RepositoryAccess::Maintained)
    );
    assert!(after.starts_with("repositories = [{"), "{after}");
}

#[test]
fn adding_a_repository_twice_keeps_one_and_opens_it() {
    let (folders, root) = temp_folders("add-twice");
    let mut files = SettingsFiles::load(folders.clone());
    drop(
        files
            .add_repository(
                &RepositorySettings::added(repository("python/cpython"), RepositoryAccess::Other),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    drop(
        files
            .add_repository(
                &RepositorySettings::added(repository("a/b"), RepositoryAccess::Other),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let before = fs::read_to_string(folders.settings_file()).unwrap();

    let home = files
        .add_repository(
            &RepositorySettings::added(repository("Python/CPython"), RepositoryAccess::Other),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(after, before);
    assert_eq!(
        home,
        opened(&["python/cpython", "a/b"], Conventions::Cpython)
    );
}

#[test]
fn adding_puts_the_repository_first_in_recent_with_the_time() {
    let (folders, root) = temp_folders("recent");
    let mut files = SettingsFiles::load(folders.clone());

    add_all(
        &mut files,
        &["a/first", "a/second", "A/First"],
        at("2026-11-10T09:00:00.5+03:00"),
    );
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let settings = fs::read_to_string(folders.settings_file()).unwrap();
    let home = files.home();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("reopen");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);

    let reopened = SettingsFiles::load(folders).home();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(
        reopened,
        opened(&["a/second", "a/first"], Conventions::Github)
    );
}

#[test]
fn a_launch_skips_a_name_the_settings_no_longer_hold() {
    let (folders, root) = temp_folders("reopen-skip");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second", "a/gone"], WHENEVER);
    hold_only(&folders, &["a/first", "a/second"]);

    let home = SettingsFiles::load(folders).home();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(home, opened(&["a/second", "a/first"], Conventions::Github));
}

#[test]
fn the_home_lists_the_repositories_most_recently_opened_first_then_the_others() {
    let (folders, root) = temp_folders("recent-order");
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
    fs::remove_dir_all(root).unwrap();

    // The settings no longer hold a/gone, and a/never and a/later were never opened on this machine.
    assert_eq!(
        home,
        opened(
            &["a/first", "a/third", "a/second", "a/never", "a/later"],
            Conventions::Github
        )
    );
}

#[test]
fn with_no_name_in_recent_left_a_launch_opens_the_first_repository() {
    let (folders, root) = temp_folders("reopen-gone");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/gone"], WHENEVER);
    hold_only(&folders, &["a/first", "a/second"]);

    let home = SettingsFiles::load(folders).home();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(home, opened(&["a/first", "a/second"], Conventions::Github));
}

#[test]
fn an_entry_in_recent_it_cant_read_is_passed_over_and_kept() {
    let (folders, root) = temp_folders("recent-unreadable");
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

    add_all(&mut files, &["a/second"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(
        (home, problems),
        (
            opened(&["a/second", "a/first"], Conventions::Github),
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
    let (folders, root) = temp_folders("recent-from-last");
    hold_only(&folders, &["a/first", "a/old"]);
    fs::write(
        folders.machine_file(),
        "# This laptop.\nlast_repository = \"a/old\" # Mine.\neditor = \"code\"\n",
    )
    .unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/new"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(reopened, opened(&["a/old", "a/first"], Conventions::Github));
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
    let (folders, root) = temp_folders("recent-from-last-again");
    fs::write(folders.machine_file(), "last_repository = \"a/old\"\n").unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    add_all(&mut files, &["a/Old"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert!(
        machine.starts_with(
            "recent = [\n    { name = \"a/Old\", opened_at = 2026-11-10T09:00:00Z },\n]\n"
        ),
        "{machine}"
    );
}

#[test]
fn any_write_of_machine_toml_turns_a_last_repository_from_0_2_0_into_recent() {
    let (folders, root) = temp_folders("recent-from-last-panes");
    hold_only(&folders, &["a/first", "a/old"]);
    let mine = "# This laptop.\nlast_repository = \"a/old\" # Mine.\neditor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.save_panes(WIDE).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let reopened = SettingsFiles::load(folders).home();
    fs::remove_dir_all(root).unwrap();

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
    assert_eq!(reopened, opened(&["a/old", "a/first"], Conventions::Github));
}

#[test]
fn a_last_repository_written_by_0_2_0_after_recent_gives_way_to_the_list() {
    let (folders, root) = temp_folders("recent-and-last");
    hold_only(&folders, &["a/one", "a/old"]);
    let mine = "recent = [\n    { name = \"a/one\" },\n]\nlast_repository = \"a/old\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    files.save_panes(PaneLayout::default()).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(reopened, opened(&["a/one", "a/old"], Conventions::Github));
    assert_eq!(machine, "recent = [\n    { name = \"a/one\" },\n]\n");
}

#[test]
fn a_recent_that_isnt_a_list_gives_way_to_a_last_repository() {
    let (folders, root) = temp_folders("recent-not-a-list");
    hold_only(&folders, &["a/first", "a/old"]);
    let mine = "recent = \"a/first\" # Wrong.\nlast_repository = \"a/old\"\neditor = \"code\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/new"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(reopened, opened(&["a/old", "a/first"], Conventions::Github));
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
    let (folders, root) = temp_folders("recent-tables");
    hold_only(&folders, &["a/one", "a/two"]);
    let mine = "[[recent]]\nname = \"a/one\"\n\n[[recent]]\nname = \"a/two\"\n";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());
    let reopened = files.home();

    add_all(&mut files, &["a/three"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(reopened, opened(&["a/one", "a/two"], Conventions::Github));
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
    let (folders, root) = temp_folders("recent-comment");
    let mine = "recent = [
    { name = \"a/one\" },
    # Work.
    { name = \"a/two\" },
]
";
    fs::write(folders.machine_file(), mine).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    add_all(&mut files, &["a/two"], at("2026-11-10T09:00:00Z"));
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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
                &RepositorySettings::added(repository("a/b"), RepositoryAccess::Other),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap(),
    );
    let text = fs::read_to_string(&moved).unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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
        let (folders, root) = temp_folders(name);
        fs::write(folders.settings_file(), settings).unwrap();
        fs::write(folders.machine_file(), machine).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files
            .add_repository(
                &RepositorySettings::added(repository("a/b"), RepositoryAccess::Other),
                &MirrorSource::Download,
                WHENEVER,
            )
            .unwrap_err();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("add-machine-readonly");
    let file = folders.machine_file();
    fs::write(&file, "mine = 1\n").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    let refused = files
        .add_repository(
            &RepositorySettings::added(repository("a/b"), RepositoryAccess::Other),
            &MirrorSource::Download,
            WHENEVER,
        )
        .unwrap_err();
    let added = folders.settings_file().exists();
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("open");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(
        &mut files,
        &["a/first", "a/second"],
        at("2026-11-10T07:00:00Z"),
    );
    let settings = fs::read_to_string(folders.settings_file()).unwrap();

    let home = files
        .open_repository(&repository("A/First"), at("2026-11-10T09:00:00.5+01:00"))
        .unwrap();
    let machine = fs::read_to_string(folders.machine_file()).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let relaunched = SettingsFiles::load(folders).home();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("open-unknown");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    let refused = files
        .open_repository(&repository("a/gone"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("open-removed");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    hold_only(&folders, &["a/second"]);
    let machine = fs::read_to_string(folders.machine_file()).unwrap();

    let refused = files
        .open_repository(&repository("a/first"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(refused.message, "Tsugiki doesn't have a/first.");
    assert_eq!(after, machine);
}

#[test]
fn opening_refuses_while_machine_toml_cant_be_used() {
    let (folders, root) = temp_folders("open-machine-broken");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    fs::write(folders.machine_file(), "settings_path =\n").unwrap();

    let refused = files
        .open_repository(&repository("a/first"), WHENEVER)
        .unwrap_err();
    let after = fs::read_to_string(folders.machine_file()).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert!(
        refused.message.contains("machine.toml, line 1"),
        "{}",
        refused.message
    );
    assert_eq!(after, "settings_path =\n");
}

#[test]
fn the_owner_opens_a_repository_and_answers_with_what_the_files_say() {
    let (folders, root) = temp_folders("owner-open");
    let mut files = SettingsFiles::load(folders);
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let owner = SettingsOwner::start(Ok(files));

    let saved = owner.open_repository(repository("a/first")).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(
        saved.home,
        opened(&["a/first", "a/second"], Conventions::Github)
    );
    // In the view by the time the owner answers.
    assert_eq!(owner.view(), saved);
}

#[test]
fn the_owners_check_reads_the_files_again_so_a_repository_taken_out_by_hand_isnt_held() {
    let (folders, root) = temp_folders("owner-check");
    let mut files = SettingsFiles::load(folders.clone());
    add_all(&mut files, &["a/first", "a/second"], WHENEVER);
    let owner = SettingsOwner::start(Ok(files));
    hold_only(&folders, &["a/first"]);

    let second = owner.check_name(repository("a/second"), &mut |_| Ok(found("a/second")));
    let first = owner.check_name(repository("A/First"), &mut |_| {
        panic!("GitHub was asked about a Repository Tsugiki has")
    });
    fs::remove_dir_all(root).unwrap();

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
    assert_eq!(owner.view().home, opened(&["a/first"], Conventions::Github));
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
    let (folders, root) = temp_folders("bad-name");
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
    fs::remove_dir_all(root).unwrap();

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
    let (folders, root) = temp_folders("save-repositories");
    fs::write(folders.settings_file(), before).unwrap();
    let mut files = SettingsFiles::load(folders.clone());

    files.write(&HIDDEN).unwrap();
    let after = fs::read_to_string(folders.settings_file()).unwrap();
    let held = files.loaded().settings.repositories.len();
    fs::remove_dir_all(root).unwrap();

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
    fs::remove_dir_all(root).unwrap();

    assert_eq!(files.loaded().settings, HIDDEN);
    let problem = only_problem(&files);
    assert!(problem.contains("dotfiles.toml"), "{problem}");
    assert!(problem.contains("last values"), "{problem}");
}
