//! Looks at the database file before SQLite opens it, so a file Tsugiki won't use stays as it is. Reading
//! the bytes leaves no trace, where even a read-only connection to a WAL database creates `-wal` and `-shm`
//! files.
//!
//! The header holds the file's schema version. SQLite sets aside one integer there, `user_version`, for the
//! program using the file, and doesn't use it itself. Tsugiki stores the number of the last migration the
//! file has run, so 0 means a new file. This number says which tables the file has, and it has nothing to do
//! with Tsugiki's app version. A number above the last migration this build knows means a newer Tsugiki
//! changed the tables in ways this build can't read.
//!
//! This only sees the header. SQLite reports damage further into the file when Tsugiki reads that part, and a
//! full check at start-up would have to read the whole file.

use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::Path;

use super::NEWER;
use crate::error::{Error, ErrorKind};

/// Where the database header section of SQLite's file format document puts these.
const HEADER_LENGTH: u16 = 100;
const SIGNATURE: &[u8; 16] = b"SQLite format 3\0";
const USER_VERSION: Range<usize> = 60..64;

const NOT_A_DATABASE: &str =
    "Tsugiki's database file is damaged or isn't a database, so Tsugiki won't change it.";

/// What the start of a file says it is.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Contents {
    /// Missing, empty or unreadable. SQLite then creates the file, or says why it can't.
    Nothing,
    Database {
        version: u32,
    },
    SomethingElse,
}

pub(super) fn contents(path: &Path) -> Contents {
    let mut header = Vec::new();
    let read = File::open(path)
        .and_then(|file| file.take(u64::from(HEADER_LENGTH)).read_to_end(&mut header));
    if !read.is_ok_and(|length| length > 0) {
        return Contents::Nothing;
    }
    let is_header = header.len() == usize::from(HEADER_LENGTH) && header.starts_with(SIGNATURE);
    is_header
        .then(|| header.get(USER_VERSION)?.try_into().ok())
        .flatten()
        .map_or(Contents::SomethingElse, |version| Contents::Database {
            version: u32::from_be_bytes(version),
        })
}

/// Refuses a file that a Tsugiki knowing migrations up to `latest` shouldn't open.
///
/// A crash between a migration's commit and its checkpoint leaves a newer version in the WAL only, where this
/// can't see it. `migrate` catches that once the file is open.
pub(super) fn check(path: &Path, latest: u32) -> Result<(), Error> {
    match contents(path) {
        Contents::Database { version } if version > latest => {
            Err(Error::new(ErrorKind::Internal, NEWER))
        }
        Contents::SomethingElse => Err(Error::new(ErrorKind::Internal, NOT_A_DATABASE)),
        Contents::Nothing | Contents::Database { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{V1, V2, backups, temp_database};
    use super::super::{CANT_OPEN, Store, open_with};
    use super::*;
    use std::ffi::OsString;
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
        let names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["tsugiki.db"], "no -wal or -shm files either");
        assert_eq!(backups(&folder), [""; 0]);
        fs::remove_dir_all(folder).unwrap();
    }

    /// What the folder holds, so a test can see that SQLite never touched the file.
    fn files_in(folder: &Path) -> Vec<OsString> {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
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
        assert_eq!(files_in(&folder), ["tsugiki.db"], "SQLite never opened it");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_too_short_to_hold_a_header_is_refused_on_every_os() {
        let (path, folder) = temp_database("short");
        for length in [1, 2, 20, 99] {
            fs::write(&path, "x".repeat(length)).unwrap();
            let problem = Store::open(&path).err().unwrap();
            assert_eq!(problem.message, NOT_A_DATABASE, "{length} bytes");
            assert_eq!(fs::read(&path).unwrap().len(), length);
        }
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_signature_alone_doesnt_make_a_database() {
        let (path, folder) = temp_database("signature-only");
        fs::write(&path, b"SQLite format 3\0 and then it stops").unwrap();

        let problem = Store::open(&path).err().unwrap();

        assert_eq!(problem.message, NOT_A_DATABASE);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_empty_file_counts_as_a_new_database() {
        let (path, folder) = temp_database("empty");
        fs::write(&path, "").unwrap();
        assert_eq!(contents(&path), Contents::Nothing);

        open_with(&path, &[V1]).unwrap().close();

        assert_eq!(contents(&path), Contents::Database { version: 1 });
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_missing_file_counts_as_a_new_database() {
        let (path, folder) = temp_database("missing");
        assert_eq!(contents(&path), Contents::Nothing);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_folder_in_the_files_place_is_left_for_sqlite_to_refuse() {
        let (path, folder) = temp_database("folder-in-place");
        fs::create_dir(&path).unwrap();

        let problem = Store::open(&path).err().unwrap();

        assert_eq!(problem.message, CANT_OPEN);
        fs::remove_dir_all(folder).unwrap();
    }
}
