use super::*;
use crate::testing::{TempPath, files_in, temp_path};

const BEFORE: &str = "[status_bar]\nvisible = true\n";
const AFTER: &str = "[status_bar]\nvisible = false\n";

/// A folder for the test, holding `settings.toml` with [`BEFORE`], and the file's path.
fn saved_before(name: &str) -> (PathBuf, TempPath) {
    let folder = temp_path(&format!("saving-{name}"));
    fs::create_dir_all(&folder).unwrap();
    let file = folder.join("settings.toml");
    fs::write(&file, BEFORE).unwrap();
    (file, folder)
}

/// A second name for the old file still reads the old text, so the save wrote none of the new into it: a
/// process that ended part way would have left it whole. A hard link into a dotfiles repository breaks so.
#[test]
fn a_save_writes_nothing_into_the_old_file_and_puts_a_new_one_in_its_place() {
    let (file, folder) = saved_before("whole");
    let held = folder.join("held.toml");
    fs::hard_link(&file, &held).unwrap();

    save(&file, AFTER).unwrap();

    assert_eq!(fs::read_to_string(&held).unwrap(), BEFORE);
    assert_eq!(fs::read_to_string(&file).unwrap(), AFTER);
    assert_eq!(files_in(&folder), ["held.toml", "settings.toml"]);
}

#[test]
fn a_save_makes_the_folder_and_the_file_when_neither_is_there() {
    let folder = temp_path("saving-new");
    let file = folder.join("settings").join("settings.toml");

    save(&file, AFTER).unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), AFTER);
    assert_eq!(files_in(&folder.join("settings")), ["settings.toml"]);
}

#[test]
fn a_read_only_file_is_refused_and_stays_as_it_was() {
    let (file, folder) = saved_before("read-only");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();

    let refused = save(&file, AFTER).unwrap_err();

    let kept = fs::read_to_string(&file).unwrap();
    let names = files_in(&folder);
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test made the file read-only, and puts it back so its folder can go"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(kept, BEFORE);
    assert_eq!(names, ["settings.toml"]);
}

/// What a save that ended between its write and its rename left, here a link someone put at the name, goes.
#[test]
fn a_save_replaces_what_a_save_that_ended_part_way_left() {
    let (file, folder) = saved_before("left");
    let elsewhere = folder.join("elsewhere.toml");
    fs::write(&elsewhere, BEFORE).unwrap();
    fs::hard_link(&elsewhere, folder.join(".settings.toml.tmp")).unwrap();

    save(&file, AFTER).unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), AFTER);
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), BEFORE);
    assert_eq!(files_in(&folder), ["elsewhere.toml", "settings.toml"]);
}

/// A folder Tsugiki can't write to fails the new file, though the settings file in it is writable.
#[cfg(unix)]
#[test]
fn a_save_that_fails_leaves_the_file_as_it_was_and_no_new_file() {
    use std::os::unix::fs::PermissionsExt as _;

    let (file, folder) = saved_before("fails");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();

    let failed = save(&file, AFTER);

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&file).unwrap(), BEFORE);
    assert_eq!(files_in(&folder), ["settings.toml"]);
}

/// The settings file and the dotfiles folder its link points into, with [`BEFORE`] in the file when `there`.
#[cfg(unix)]
fn linked(name: &str, there: bool) -> (PathBuf, PathBuf, TempPath) {
    let folder = temp_path(&format!("saving-{name}"));
    let dotfiles = folder.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(folder.join("settings")).unwrap();
    let shared = dotfiles.join("settings.toml");
    if there {
        fs::write(&shared, BEFORE).unwrap();
    }
    let link = folder.join("settings").join("settings.toml");
    std::os::unix::fs::symlink(&shared, &link).unwrap();
    (link, shared, folder)
}

/// Windows makes a symbolic link only for an administrator or in Developer Mode, so this runs on macOS.
#[cfg(unix)]
#[test]
fn a_save_through_a_symbolic_link_keeps_the_link_and_changes_the_file_it_points_to() {
    let (link, shared, folder) = linked("link", true);

    save(&link, AFTER).unwrap();

    assert_eq!(fs::read_link(&link).unwrap(), shared);
    assert_eq!(fs::read_to_string(&shared).unwrap(), AFTER);
    assert_eq!(files_in(&folder.join("settings")), ["settings.toml"]);
    assert_eq!(files_in(&folder.join("dotfiles")), ["settings.toml"]);
}

/// As when the dotfiles repository isn't cloned yet, or its drive isn't there.
#[cfg(unix)]
#[test]
fn a_save_through_a_symbolic_link_to_no_file_is_refused_and_keeps_the_link() {
    let (link, shared, folder) = linked("dangling", false);

    let refused = save(&link, AFTER).unwrap_err();

    assert_eq!(refused.kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read_link(&link).unwrap(), shared);
    assert_eq!(files_in(&folder.join("settings")), ["settings.toml"]);
    assert_eq!(files_in(&folder.join("dotfiles")), [""; 0]);
}

#[cfg(unix)]
#[test]
fn a_save_keeps_the_files_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let (file, _folder) = saved_before("mode");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();

    save(&file, AFTER).unwrap();

    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

/// `settings.toml` opened as a program that reads it and won't let it be deleted meanwhile opens it.
#[cfg(windows)]
fn held_open(file: &Path) -> File {
    use std::os::windows::fs::OpenOptionsExt as _;

    /// `FILE_SHARE_READ | FILE_SHARE_WRITE`, without `FILE_SHARE_DELETE`.
    const READ_AND_WRITE: u32 = 0x1 | 0x2;
    OpenOptions::new()
        .read(true)
        .share_mode(READ_AND_WRITE)
        .open(file)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn a_save_waits_for_a_program_that_holds_the_file_for_a_moment() {
    let (file, folder) = saved_before("held");
    let held = held_open(&file);
    let letting_go = thread::spawn(move || {
        thread::sleep(TRY_AGAIN * 2);
        drop(held);
    });

    let saved = save(&file, AFTER);

    letting_go.join().unwrap();
    saved.unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), AFTER);
    assert_eq!(files_in(&folder), ["settings.toml"]);
}

#[cfg(windows)]
#[test]
fn a_save_gives_up_on_a_program_that_holds_the_file_and_leaves_no_new_file() {
    let (file, folder) = saved_before("held-long");
    let held = held_open(&file);

    let failed = save(&file, AFTER);

    drop(held);
    assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&file).unwrap(), BEFORE);
    assert_eq!(files_in(&folder), ["settings.toml"]);
}
