use super::*;
use crate::testing::{TempPath, files_in, temp_path};
#[cfg(windows)]
use tsugiki_test_git::{SHARE_READ, SHARE_WRITE, held_open};

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
    let left = format!(".settings.toml.{}.tmp", std::process::id());
    fs::hard_link(&elsewhere, folder.join(left)).unwrap();

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

/// The settings file, a link to a file in a dotfiles folder that isn't there yet, and that file's path.
#[cfg(unix)]
fn linked(name: &str) -> (PathBuf, PathBuf, TempPath) {
    let folder = temp_path(&format!("saving-{name}"));
    let dotfiles = folder.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(folder.join("settings")).unwrap();
    let shared = dotfiles.join("settings.toml");
    let link = folder.join("settings").join("settings.toml");
    std::os::unix::fs::symlink(&shared, &link).unwrap();
    (link, shared, folder)
}

/// Windows makes a symbolic link only for an administrator or in Developer Mode, so this runs on macOS.
#[cfg(unix)]
#[test]
fn a_save_through_a_symbolic_link_keeps_the_link_and_changes_the_file_it_points_to() {
    let (link, shared, folder) = linked("link");
    fs::write(&shared, BEFORE).unwrap();

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
    let (link, shared, folder) = linked("dangling");

    let refused = save(&link, AFTER).unwrap_err();

    assert_eq!(refused.kind(), io::ErrorKind::NotFound);
    let message = refused.to_string();
    assert!(
        message.starts_with(&format!("it links to {}: ", shared.display())),
        "{message}"
    );
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

/// What a save of a file that a program shares as `share` says writes, once the program lets go after a
/// moment, and what the folder holds then.
#[cfg(windows)]
fn saved_once_let_go(name: &str, share: u32) -> (String, Vec<OsString>) {
    let (file, folder) = saved_before(name);
    let held = held_open(&file, share);
    let letting_go = thread::spawn(move || {
        thread::sleep(TRY_AGAIN * 2);
        drop(held);
    });

    let saved = save(&file, AFTER);

    letting_go.join().unwrap();
    saved.unwrap();
    (fs::read_to_string(&file).unwrap(), files_in(&folder))
}

/// The rename waits for it, as the program doesn't share deletion.
#[cfg(windows)]
#[test]
fn a_save_waits_for_a_program_that_reads_the_file_for_a_moment() {
    let saved = saved_once_let_go("held", SHARE_READ | SHARE_WRITE);

    assert_eq!(saved, (AFTER.to_owned(), vec!["settings.toml".into()]));
}

/// Opening the file to write waits for it, as the program shares neither writing nor deletion.
#[cfg(windows)]
#[test]
fn a_save_waits_for_a_program_that_holds_the_file_from_writers_for_a_moment() {
    let saved = saved_once_let_go("held-from-writers", SHARE_READ);

    assert_eq!(saved, (AFTER.to_owned(), vec!["settings.toml".into()]));
}

#[cfg(windows)]
#[test]
fn a_save_gives_up_on_a_program_that_holds_the_file_and_leaves_no_new_file() {
    let (file, folder) = saved_before("held-long");
    let held = held_open(&file, SHARE_READ | SHARE_WRITE);

    let failed = save(&file, AFTER);

    drop(held);
    assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&file).unwrap(), BEFORE);
    assert_eq!(files_in(&folder), ["settings.toml"]);
}
