//! How much of the disk a folder of Tsugiki's takes, such as a Repository's Mirror, which Settings ›
//! Repositories shows (#533).

use std::fs::{self, Metadata};
use std::path::Path;

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
}

impl FolderSize {
    /// How much of the disk `folder` takes now: the length of each file under it, in its folders too. A link
    /// counts as itself, so what it points to doesn't count twice. A file that goes while this counts, as
    /// git's upkeep deletes an old pack, counts as nothing, and so does one it can't read: the size is for
    /// the user to read, not to act on. It reads the disk, so the window asks for it off its own thread.
    pub fn of(folder: &Path) -> Self {
        if !folder.is_dir() {
            return Self::Missing;
        }
        let mut bytes = 0_u64;
        let mut folders = vec![folder.to_owned()];
        while let Some(next) = folders.pop() {
            for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    folders.push(entry.path());
                } else {
                    let length = entry.metadata().as_ref().map_or(0, Metadata::len);
                    bytes = bytes.saturating_add(length);
                }
            }
        }
        Self::Measured {
            mb: megabytes(bytes),
        }
    }
}

/// `bytes` in whole megabytes, at most `u32::MAX`, which the window shows as it is.
pub(crate) fn megabytes(bytes: u64) -> u32 {
    u32::try_from(bytes >> 20).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
