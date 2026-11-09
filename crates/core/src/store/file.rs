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
mod tests;
