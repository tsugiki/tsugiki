use super::*;
use std::fs;

/// A new folder under `.temp`, with a path that is absolute on this OS.
fn temp_folder(name: &str) -> PathBuf {
    let folder = crate::testing::temp_path(&format!("folders-{name}"));
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// An environment with the given variables, and no others.
fn env_of(variables: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let variables: Vec<(String, OsString)> = variables
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.as_os_str().to_owned()))
        .collect();
    move |wanted| {
        variables
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn an_installed_release_logs_beside_its_database() {
    let root = temp_folder("release-log");
    let (local, roaming) = (root.join("Local"), root.join("Roaming"));
    let env = env_of(&[
        ("LOCALAPPDATA", &local),
        ("APPDATA", &roaming),
        ("HOME", &root),
    ]);

    let windows = Folders::find(Os::Windows, Build::Release, &root.join("tsg.exe"), &env).unwrap();
    let macos = Folders::find(Os::MacOs, Build::Release, &root.join("tsg"), &env).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(windows.log(), local.join("tsugiki").join("tsugiki.log"));
    assert_eq!(
        macos.log(),
        root.join("Library")
            .join("Application Support")
            .join("tsugiki")
            .join("tsugiki.log")
    );
}

#[test]
fn a_development_build_logs_in_the_repos_root_where_just_dev_runs() {
    let root = temp_folder("development-log");
    let env = env_of(&[("LOCALAPPDATA", &root), ("APPDATA", &root), ("HOME", &root)]);

    let windows =
        Folders::find(Os::Windows, Build::Development, &root.join("tsg.exe"), &env).unwrap();
    let macos = Folders::find(Os::MacOs, Build::Development, &root.join("tsg"), &env).unwrap();
    fs::remove_dir_all(&root).unwrap();

    for folders in [windows, macos] {
        let log = folders.log();
        assert_eq!(log.file_name().unwrap(), "tsugiki.log");
        let folder = log.parent().unwrap();
        assert!(folder.join("justfile").is_file(), "{}", log.display());
        assert!(folder.join("Cargo.lock").is_file(), "{}", log.display());
        assert!(folders.data.ends_with("tsugiki-dev"), "{:?}", folders.data);
    }
}

#[test]
fn a_checkout_that_moved_or_went_leaves_no_repos_root_to_log_in() {
    let root = temp_folder("moved-checkout");
    let crate_folder = root.join("crates").join("core");
    fs::create_dir_all(&crate_folder).unwrap();

    let gone = checkout_root(&crate_folder);
    fs::write(root.join("justfile"), "").unwrap();
    let there = checkout_root(&crate_folder);
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(gone, None);
    assert_eq!(there, Some(root));
}

#[test]
fn a_portable_copy_logs_in_its_folder_whatever_its_build() {
    let root = temp_folder("portable-log");
    fs::create_dir(root.join("data")).unwrap();
    let exe = root.join("tsg.exe");

    let release = Folders::find(Os::Windows, Build::Release, &exe, &env_of(&[])).unwrap();
    let development = Folders::find(Os::Windows, Build::Development, &exe, &env_of(&[])).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(release.log(), root.join("data").join("tsugiki.log"));
    assert_eq!(development.log(), root.join("data").join("tsugiki.log"));
}

#[test]
fn the_webview_keeps_its_data_with_ours_so_a_portable_copy_stays_in_its_folder() {
    let folders = Folders {
        data: PathBuf::from("data"),
        settings: PathBuf::from("settings"),
        logs: PathBuf::from("logs"),
    };
    assert_eq!(folders.webview(), Path::new("data").join("webview"));
}

#[test]
fn the_current_folders_come_from_this_program_and_its_real_environment() {
    let folders = Folders::current().unwrap();
    assert!(folders.database().is_absolute());
    assert!(folders.settings.is_absolute());
}

#[test]
fn a_program_that_cant_tell_where_it_is_says_so() {
    let lost = Err(std::io::Error::from(std::io::ErrorKind::NotFound));

    let problem = Folders::from_exe(lost, &env_of(&[])).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert_eq!(
        problem.message,
        "Tsugiki can't tell where it is running from."
    );
}

#[test]
fn installed_on_windows_data_is_local_and_settings_roam() {
    let root = temp_folder("windows-installed");
    let (local, roaming) = (root.join("Local"), root.join("Roaming"));
    let env = env_of(&[("LOCALAPPDATA", &local), ("APPDATA", &roaming)]);
    let exe = local.join("Programs").join("Tsugiki").join("tsg.exe");

    let folders = Folders::find(Os::Windows, Build::Release, &exe, &env).unwrap();
    fs::remove_dir_all(root).unwrap();

    assert_eq!(folders.data, local.join("tsugiki"));
    assert_eq!(folders.settings, roaming.join("tsugiki"));
}

#[test]
fn installed_on_macos_everything_is_in_application_support() {
    let home = temp_folder("macos-installed");
    let env = env_of(&[("HOME", &home)]);
    let exe = home
        .join("Tsugiki.app")
        .join("Contents")
        .join("MacOS")
        .join("tsg");

    let folders = Folders::find(Os::MacOs, Build::Release, &exe, &env).unwrap();
    fs::remove_dir_all(&home).unwrap();

    let expected = home
        .join("Library")
        .join("Application Support")
        .join("tsugiki");
    assert_eq!(folders.data, expected);
    assert_eq!(folders.settings, expected);
}

#[test]
fn a_development_build_stays_out_of_the_installed_folders() {
    let root = temp_folder("development");
    let (local, roaming) = (root.join("Local"), root.join("Roaming"));
    let env = env_of(&[
        ("LOCALAPPDATA", &local),
        ("APPDATA", &roaming),
        ("HOME", &root),
    ]);

    let windows =
        Folders::find(Os::Windows, Build::Development, &root.join("tsg.exe"), &env).unwrap();
    let macos = Folders::find(Os::MacOs, Build::Development, &root.join("tsg"), &env).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(windows.data, local.join("tsugiki-dev"));
    assert_eq!(windows.settings, roaming.join("tsugiki-dev"));
    assert_eq!(
        macos.data,
        root.join("Library")
            .join("Application Support")
            .join("tsugiki-dev")
    );
}

#[test]
fn a_development_build_can_still_be_portable() {
    let root = temp_folder("development-portable");
    fs::create_dir(root.join("data")).unwrap();

    let folders = Folders::find(
        Os::Windows,
        Build::Development,
        &root.join("tsg.exe"),
        &env_of(&[]),
    )
    .unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(folders.data, root.join("data"));
}

#[test]
fn a_data_folder_next_to_tsg_exe_makes_windows_portable() {
    let root = temp_folder("windows-portable");
    fs::create_dir(root.join("data")).unwrap();
    let no_variables = env_of(&[]);

    let folders = Folders::find(
        Os::Windows,
        Build::Release,
        &root.join("tsg.exe"),
        &no_variables,
    )
    .unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(folders.data, root.join("data"));
    assert_eq!(folders.settings, root.join("data"));
}

#[test]
fn a_folder_next_to_the_app_bundle_makes_macos_portable() {
    let root = temp_folder("macos-portable");
    let portable = root.join("tsugiki-portable-data");
    fs::create_dir(&portable).unwrap();
    let exe = root
        .join("Tsugiki.app")
        .join("Contents")
        .join("MacOS")
        .join("tsg");

    let folders = Folders::find(Os::MacOs, Build::Release, &exe, &env_of(&[])).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(folders.data, portable);
    assert_eq!(folders.settings, portable);
}

#[test]
fn a_macos_program_outside_a_bundle_looks_next_to_itself() {
    let root = temp_folder("macos-bare");
    let portable = root.join("tsugiki-portable-data");
    fs::create_dir(&portable).unwrap();

    let folders =
        Folders::find(Os::MacOs, Build::Release, &root.join("tsg"), &env_of(&[])).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(folders.data, portable);
}

#[test]
fn a_file_named_data_doesnt_make_tsugiki_portable() {
    let root = temp_folder("file-named-data");
    fs::write(root.join("data"), "").unwrap();
    let env = env_of(&[("LOCALAPPDATA", &root), ("APPDATA", &root)]);

    let folders = Folders::find(Os::Windows, Build::Release, &root.join("tsg.exe"), &env).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(folders.data, root.join("tsugiki"));
}

#[test]
fn each_os_looks_only_for_its_own_portable_folder() {
    let root = temp_folder("other-os-portable");
    let env = env_of(&[("HOME", &root), ("LOCALAPPDATA", &root), ("APPDATA", &root)]);

    fs::create_dir(root.join("data")).unwrap();
    let macos = Folders::find(Os::MacOs, Build::Release, &root.join("tsg"), &env).unwrap();
    fs::remove_dir(root.join("data")).unwrap();

    fs::create_dir(root.join("tsugiki-portable-data")).unwrap();
    let windows = Folders::find(Os::Windows, Build::Release, &root.join("tsg.exe"), &env).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(
        macos.data,
        root.join("Library")
            .join("Application Support")
            .join("tsugiki")
    );
    assert_eq!(windows.data, root.join("tsugiki"));
}

#[test]
fn a_missing_variable_is_an_error_that_names_it() {
    let root = temp_folder("missing");
    let only_local = env_of(&[("LOCALAPPDATA", &root)]);
    let exe = root.join("tsg.exe");

    let windows = Folders::find(Os::Windows, Build::Release, &exe, &only_local).unwrap_err();
    let macos = Folders::find(Os::MacOs, Build::Release, &exe, &only_local).unwrap_err();
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(windows.kind, ErrorKind::Internal);
    assert!(windows.message.contains(" APPDATA "), "{}", windows.message);
    assert!(macos.message.contains(" HOME "), "{}", macos.message);
}

#[test]
fn an_empty_or_relative_variable_counts_as_missing() {
    // A relative path would put the database wherever Tsugiki happened to start.
    let root = temp_folder("relative");
    let exe = root.join("tsg");
    let relative = env_of(&[("HOME", Path::new("relative"))]);
    let empty = env_of(&[("HOME", Path::new(""))]);

    let relative = Folders::find(Os::MacOs, Build::Release, &exe, &relative).unwrap_err();
    let empty = Folders::find(Os::MacOs, Build::Release, &exe, &empty).unwrap_err();
    fs::remove_dir_all(&root).unwrap();

    assert!(relative.message.contains("HOME"), "{}", relative.message);
    assert!(empty.message.contains("HOME"), "{}", empty.message);
}
