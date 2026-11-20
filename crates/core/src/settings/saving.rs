//! Saving a settings file whole: a process that ends part way through a save leaves the old text or the new,
//! never a file cut short (#939).

use std::ffi::OsString;
use std::fs::{self, File, Metadata};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// Writes `text` to a new file beside the one at `path`, then renames it over that one. A symbolic link, such
/// as one into a dotfiles repository, is followed first, so the link stays and the file it points to changes.
/// A link to a file that isn't there is replaced by the file.
///
/// A read-only file stays as it is, as a write over it would. A failure takes the new file away again; only a
/// process that ends between the write and the rename leaves it, beside a settings file still whole.
pub(super) fn save(path: &Path, text: &str) -> io::Result<()> {
    let target = followed(path);
    fs::create_dir_all(target.with_file_name(""))?;
    let old = match fs::metadata(&target) {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => None,
        found => Some(found?),
    };
    if old.as_ref().is_some_and(|old| old.permissions().readonly()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the file is read-only",
        ));
    }
    let new = beside(&target);
    let saved = write_new(&new, old.as_ref(), text).and_then(|()| fs::rename(&new, &target));
    if saved.is_err() {
        let _gone = fs::remove_file(&new);
    }
    saved
}

/// The file `path` names once its symbolic links are followed, or `path` itself when that fails, as it does
/// for a file that isn't there yet.
fn followed(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The path the new text goes to before the rename: in the same folder as `file`, since a rename can't cross
/// from one drive to another, and hidden on macOS. The process ID keeps two Tsugikis that share a folder, a
/// development build beside an installed one, apart.
fn beside(file: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", std::process::id()));
    file.with_file_name(name)
}

/// Writes `text` to the file at `new`, with the permissions of `old`, the file it replaces, and waits for the
/// disk to hold it, so the rename can't put a file in place whose text is still on its way.
fn write_new(new: &Path, old: Option<&Metadata>, text: &str) -> io::Result<()> {
    let mut file = File::create(new)?;
    file.write_all(text.as_bytes())?;
    if let Some(old) = old {
        keep_mode(&file, old)?;
    }
    file.sync_all()
}

/// Gives `file` the mode of `old`, which the rename would otherwise drop for the one a new file gets.
#[cfg(unix)]
fn keep_mode(file: &File, old: &Metadata) -> io::Result<()> {
    file.set_permissions(old.permissions())
}

/// Windows has no mode: `Permissions` there says whether the file is read-only, which `save` refuses before
/// this.
#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Unix version can fail, and both have one signature"
)]
const fn keep_mode(_file: &File, _old: &Metadata) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
