//! Saving a settings file whole: a process that ends part way through a save leaves the old text or the new,
//! never a file cut short (#939).

use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// How long the rename tries again while another program holds the settings file open on Windows, as an
/// antivirus, a sync client or the search indexer does for a moment. It stays under `QUIT_PATIENCE` in
/// `startup.rs`, so a save at a quit can still finish.
#[cfg(windows)]
const HELD_FOR: Duration = Duration::from_millis(500);
/// How long the rename waits between tries.
#[cfg(windows)]
const TRY_AGAIN: Duration = Duration::from_millis(50);
/// The errors Windows gives for a rename over a file another program holds open without letting it go,
/// `ERROR_ACCESS_DENIED` and `ERROR_SHARING_VIOLATION`.
#[cfg(windows)]
const HELD: [i32; 2] = [5, 32];

/// Writes `text` to a new file beside the one at `path`, then renames it over that one. A symbolic link, such
/// as one into a dotfiles repository, is followed first, so the link stays and the file it points to changes.
/// A hard link stops sharing the file at the first save, since the rename puts a new file at the path.
///
/// A file Tsugiki can't write to stays as it is, as a write over it would. A failure takes the new file away
/// again. Only a process that ends between the write and the rename leaves it, beside a settings file still
/// whole, and the next save replaces it.
pub(super) fn save(path: &Path, text: &str) -> io::Result<()> {
    let target = followed(path)?;
    fs::create_dir_all(target.with_file_name(""))?;
    // Opened to learn that Tsugiki may write there, as a rename needs only the folder's permission.
    let old = match OpenOptions::new().write(true).open(&target) {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => None,
        opened => Some(opened?.metadata()?),
    };
    let new = beside(&target);
    let saved = write_new(&new, old.as_ref(), text).and_then(|()| rename(&new, &target));
    if saved.is_err() {
        let _gone = fs::remove_file(&new);
    }
    saved
}

/// The file `path` names once its symbolic links are followed, or `path` itself when that fails for anything
/// but a link, as it does for a file that isn't there yet. A link whose file can't be found, as in a dotfiles
/// repository not cloned yet, is refused, so the rename doesn't put a file in its place.
fn followed(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path).or_else(|cause| {
        let link = fs::symlink_metadata(path)
            .ok()
            .as_ref()
            .is_some_and(Metadata::is_symlink);
        (!link).then(|| path.to_owned()).ok_or(cause)
    })
}

/// The path the new text goes to before the rename: in the same folder as `file`, since a rename can't cross
/// from one drive to another, and hidden on macOS.
fn beside(file: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(".tmp");
    file.with_file_name(name)
}

/// Writes `text` to a file of its own at `new`, with the permissions of `old`, the file it replaces, and
/// waits for the disk to hold it, so the rename can't put a file in place whose text is still on its way.
/// What a save that ended part way left at `new` goes first. `create_new` then makes sure the file is new,
/// and so not a link someone put at the name.
fn write_new(new: &Path, old: Option<&Metadata>, text: &str) -> io::Result<()> {
    let _gone = fs::remove_file(new);
    let mut file = OpenOptions::new().write(true).create_new(true).open(new)?;
    if let Some(old) = old {
        keep_mode(&file, old)?;
    }
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Gives `file` the mode of `old`, before it holds any text, which the rename would otherwise drop for the
/// one a new file gets.
#[cfg(unix)]
fn keep_mode(file: &File, old: &Metadata) -> io::Result<()> {
    file.set_permissions(old.permissions())
}

/// Windows has no mode: `Permissions` there says whether the file is read-only, and `save` has found it
/// isn't.
#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Unix version can fail, and both have one signature"
)]
const fn keep_mode(_file: &File, _old: &Metadata) -> io::Result<()> {
    Ok(())
}

/// Renames `from` over `to`, trying again for [`HELD_FOR`] while another program holds `to` open. A write
/// over the file worked while that program read it, and a rename replaces the file only once it lets go.
#[cfg(windows)]
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let give_up = Instant::now() + HELD_FOR;
    loop {
        let renamed = fs::rename(from, to);
        let code = renamed.as_ref().err().and_then(io::Error::raw_os_error);
        let held = code.is_some_and(|code| HELD.contains(&code));
        if !held || Instant::now() >= give_up {
            return renamed;
        }
        thread::sleep(TRY_AGAIN);
    }
}

/// Unix renames over a file that another program has open.
#[cfg(not(windows))]
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(test)]
mod tests;
