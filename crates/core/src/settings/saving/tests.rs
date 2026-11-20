use super::*;
use crate::testing::{TempPath, temp_path};

/// A folder for the test, holding `settings.toml` with `before`, and the file's path.
fn saved_before(name: &str, before: &str) -> (PathBuf, TempPath) {
    let folder = temp_path(&format!("saving-{name}"));
    fs::create_dir_all(&folder).unwrap();
    let file = folder.join("settings.toml");
    fs::write(&file, before).unwrap();
    (file, folder)
}

/// The names in `folder`, sorted.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A second name for the old file still reads the old text, so the save wrote none of the new into it: a
/// process that ended part way would have left it whole.
#[test]
fn a_save_writes_nothing_into_the_old_file_and_puts_a_new_one_in_its_place() {
    let (file, folder) = saved_before("whole", "[status_bar]\nvisible = true\n");
    let held = folder.join("held.toml");
    fs::hard_link(&file, &held).unwrap();

    save(&file, "[status_bar]\nvisible = false\n").unwrap();

    assert_eq!(
        fs::read_to_string(&held).unwrap(),
        "[status_bar]\nvisible = true\n"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[status_bar]\nvisible = false\n"
    );
    assert_eq!(names_in(&folder), ["held.toml", "settings.toml"]);
}

#[test]
fn a_save_makes_the_folder_and_the_file_when_neither_is_there() {
    let folder = temp_path("saving-new");
    let file = folder.join("settings").join("settings.toml");

    save(&file, "[status_bar]\nvisible = false\n").unwrap();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "[status_bar]\nvisible = false\n"
    );
    assert_eq!(names_in(&folder.join("settings")), ["settings.toml"]);
}

#[test]
fn a_read_only_file_is_refused_and_stays_as_it_was() {
    let (file, folder) = saved_before("read-only", "[status_bar]\nvisible = true\n");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();

    let refused = save(&file, "[status_bar]\nvisible = false\n").unwrap_err();

    let kept = fs::read_to_string(&file).unwrap();
    let names = names_in(&folder);
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test made the file read-only, and puts it back so its folder can go"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(kept, "[status_bar]\nvisible = true\n");
    assert_eq!(names, ["settings.toml"]);
}

/// A folder where the file should be fails the rename on each system.
#[test]
fn a_save_that_fails_leaves_no_new_file_behind() {
    let folder = temp_path("saving-fails");
    let file = folder.join("settings.toml");
    fs::create_dir_all(&file).unwrap();

    let failed = save(&file, "[status_bar]\nvisible = false\n");

    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(names_in(&folder), ["settings.toml"]);
    assert!(file.is_dir());
}

/// Windows makes a symbolic link only for an administrator or in Developer Mode, so this runs on macOS.
#[cfg(unix)]
#[test]
fn a_save_through_a_symbolic_link_keeps_the_link_and_changes_the_file_it_points_to() {
    let folder = temp_path("saving-link");
    let dotfiles = folder.join("dotfiles");
    let settings = folder.join("settings");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(&settings).unwrap();
    let shared = dotfiles.join("settings.toml");
    fs::write(&shared, "[status_bar]\nvisible = true\n").unwrap();
    let link = settings.join("settings.toml");
    std::os::unix::fs::symlink(&shared, &link).unwrap();

    save(&link, "[status_bar]\nvisible = false\n").unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link).unwrap(), shared);
    assert_eq!(
        fs::read_to_string(&shared).unwrap(),
        "[status_bar]\nvisible = false\n"
    );
    assert_eq!(names_in(&settings), ["settings.toml"]);
    assert_eq!(names_in(&dotfiles), ["settings.toml"]);
}

#[cfg(unix)]
#[test]
fn a_save_keeps_the_files_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let (file, _folder) = saved_before("mode", "[status_bar]\nvisible = true\n");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();

    save(&file, "[status_bar]\nvisible = false\n").unwrap();

    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}
