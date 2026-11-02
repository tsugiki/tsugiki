//! Looks at the database file before SQLite opens it, so a file Tsugiki won't use stays as it is.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::NEWER;
use crate::error::{Error, ErrorKind};

/// Where the database header section of SQLite's file format document puts these.
const SQLITE_SIGNATURE: &[u8; 16] = b"SQLite format 3\0";
const USER_VERSION_OFFSET: u64 = 60;

/// Refuses a file that a Tsugiki knowing migrations up to `latest` shouldn't open.
pub(super) fn check(path: &Path, latest: u32) -> Result<(), Error> {
    if version_in_header(path).is_some_and(|version| version > latest) {
        return Err(Error::new(ErrorKind::Internal, NEWER));
    }
    Ok(())
}

/// The `user_version` in a SQLite file's header. Reading the bytes leaves no trace, where even a read-only
/// connection to a WAL database creates `-wal` and `-shm` files. Any other file has no version to read.
///
/// A crash between a migration's commit and its checkpoint leaves a newer version in the WAL only. `migrate`
/// catches that once the file is open, and closing that connection then moves the WAL into the file.
pub(super) fn version_in_header(path: &Path) -> Option<u32> {
    let mut file = File::open(path).ok()?;
    let mut signature = [0; SQLITE_SIGNATURE.len()];
    file.read_exact(&mut signature).ok()?;
    let _position = file.seek(SeekFrom::Start(USER_VERSION_OFFSET)).ok()?;
    let mut version = [0; 4];
    file.read_exact(&mut version).ok()?;
    (signature == *SQLITE_SIGNATURE).then_some(u32::from_be_bytes(version))
}

#[cfg(test)]
mod tests {
    use super::super::testing::{V1, V2, backups, temp_database};
    use super::super::{CANT_OPEN, Store, open_with};
    use crate::error::ErrorKind;
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

    #[test]
    fn a_file_that_isnt_a_database_is_an_error_and_left_untouched() {
        let (path, folder) = temp_database("not-a-database");
        let text = "this is not a database, and it is long enough for SQLite to look at".repeat(9);
        fs::write(&path, &text).unwrap();

        let problem = Store::open(&path).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert_eq!(problem.message, CANT_OPEN, "plain words, not SQLite's");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_too_short_to_be_a_database_is_not_called_newer() {
        let (path, folder) = temp_database("short");
        // Not 1 byte: SQLite on Unix reports that size as 0 and treats the file as empty (its ticket #3260).
        for length in [2, 20] {
            fs::write(&path, "x".repeat(length)).unwrap();
            let problem = Store::open(&path).err().unwrap();
            assert!(!problem.message.contains("newer"), "{}", problem.message);
        }
        fs::remove_dir_all(folder).unwrap();
    }
}
