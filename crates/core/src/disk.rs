//! How much of the disk a folder of Tsugiki's takes, such as a Repository's Mirror, which Settings ›
//! Repositories shows (#533).

use std::fs::{self, Metadata, ReadDir};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;

/// How much of the disk a folder takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FolderSize {
    /// There is no folder, as before a Mirror's first clone has finished.
    Missing,
    /// Its files take `mb` whole megabytes.
    Measured { mb: u32 },
    /// Something is there, but it can't be read as a folder, so its size is unknown.
    Unreadable,
}

impl FolderSize {
    /// How much of the disk `folder` takes now: the length of each file under it, in its folders too. A link
    /// counts as itself, so what it points to doesn't count twice. A file or a folder inside it that goes
    /// while this counts, as git's upkeep deletes an old pack, counts as nothing, and so does one it can't
    /// read: the size is for the user to read, not to act on. It reads the disk, so the window asks for it
    /// off its own thread.
    pub fn of(folder: &Path) -> Self {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(cause) if cause.kind() == ErrorKind::NotFound => return Self::Missing,
            Err(_) => return Self::Unreadable,
        };
        let mut bytes = 0_u64;
        let mut folders = Vec::new();
        count(entries, &mut bytes, &mut folders);
        while let Some(next) = folders.pop() {
            if let Ok(entries) = fs::read_dir(&next) {
                count(entries, &mut bytes, &mut folders);
            }
        }
        Self::Measured {
            mb: megabytes(bytes),
        }
    }
}

/// Adds the length of each file in `entries` to `bytes`, and puts each folder among them on `folders`.
fn count(entries: ReadDir, bytes: &mut u64, folders: &mut Vec<PathBuf>) {
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            folders.push(entry.path());
        } else {
            let length = entry.metadata().as_ref().map_or(0, Metadata::len);
            *bytes = bytes.saturating_add(length);
        }
    }
}

/// `bytes` in whole megabytes, at most `u32::MAX`, which the window shows as it is.
pub(crate) fn megabytes(bytes: u64) -> u32 {
    u32::try_from(bytes >> 20).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
