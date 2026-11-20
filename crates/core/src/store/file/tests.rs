use super::super::testing::{V1, V2, backups, temp_database};
use super::super::{CANT_OPEN, Store, open_with};
use super::*;
use crate::testing::files_in;
use std::fs;

#[test]
fn a_newer_database_is_refused_and_left_untouched() {
    let (path, folder) = temp_database("newer");
    open_with(&path, &[V1, V2]).unwrap().close();
    let before = fs::read(&path).unwrap();

    let problem = open_with(&path, &[V1]).err().unwrap();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert!(problem.message.contains("newer"), "{}", problem.message);
    assert_eq!(fs::read(&path).unwrap(), before);
    // The lock is Tsugiki's own, beside the file.
    assert_eq!(
        files_in(&folder),
        ["tsugiki.db", "tsugiki.db.lock"],
        "no -wal or -shm files either"
    );
    assert_eq!(backups(&folder), [""; 0]);
}

#[test]
fn a_file_that_isnt_a_database_is_refused_and_left_untouched() {
    let (path, folder) = temp_database("not-a-database");
    let text = "this is not a database, and it is long enough for a whole header".repeat(9);
    fs::write(&path, &text).unwrap();

    let problem = Store::open(&path).err().unwrap();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert_eq!(problem.message, NOT_A_DATABASE);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let left = files_in(&folder);
    assert_eq!(
        left,
        ["tsugiki.db", "tsugiki.db.lock"],
        "SQLite never opened it"
    );
}

#[test]
fn a_file_too_short_to_hold_a_header_is_refused_on_every_os() {
    let (path, _folder) = temp_database("short");
    for length in [1, 2, 20, 99] {
        fs::write(&path, "x".repeat(length)).unwrap();
        let problem = Store::open(&path).err().unwrap();
        assert_eq!(problem.message, NOT_A_DATABASE, "{length} bytes");
        assert_eq!(fs::read(&path).unwrap().len(), length);
    }
}

#[test]
fn a_signature_without_a_whole_header_doesnt_make_a_database() {
    let (path, _folder) = temp_database("signature-only");
    // Long enough to hold the user version, and one byte short of a header.
    let mut short = SIGNATURE.to_vec();
    short.resize(usize::from(HEADER_LENGTH) - 1, 0);
    fs::write(&path, short).unwrap();

    let problem = Store::open(&path).err().unwrap();

    assert_eq!(problem.message, NOT_A_DATABASE);
}

#[test]
fn an_empty_file_counts_as_a_new_database() {
    let (path, _folder) = temp_database("empty");
    fs::write(&path, "").unwrap();
    assert_eq!(contents(&path), Contents::Nothing);

    open_with(&path, &[V1]).unwrap().close();

    assert_eq!(contents(&path), Contents::Database { version: 1 });
}

#[test]
fn a_folder_in_the_files_place_is_left_for_sqlite_to_refuse() {
    let (path, _folder) = temp_database("folder-in-place");
    fs::create_dir(&path).unwrap();

    let problem = Store::open(&path).err().unwrap();

    assert_eq!(problem.message, CANT_OPEN);
}
