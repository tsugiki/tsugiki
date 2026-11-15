//! Git repositories for the tests of `tsugiki-core` and `tsugiki-app`, built with the real git (ADR 0009),
//! and the folders under `.temp` that the tests of those and `tsugiki-lsp` work in. Core's `testing`
//! modules are private to core, so the helpers the crates' tests share live here, and a change to how a
//! test runs git or where it keeps its files is made once.

#![expect(
    clippy::disallowed_types,
    reason = "tests build their repositories with the real git (ADR 0009)"
)]
#![expect(clippy::unwrap_used, reason = "a failed setup step ends the test")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use std::{fs, io, iter, thread};

/// The author of each commit a test makes, so a commit needs no git config on the machine.
const AUTHOR: [&str; 4] = [
    "-c",
    "user.name=Contributor",
    "-c",
    "user.email=contributor@example.com",
];

/// What points git at another repository than the one in the folder it runs in. A hook or an alias that
/// runs the tests can leave these set, and then `reset --hard` would act on the developer's checkout.
const ELSEWHERE: [&str; 3] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"];

/// Runs git in `dir` and gives what it printed. A git that fails ends the test with what it said.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for name in ELSEWHERE {
        let _ = command.env_remove(name);
    }
    let output = command
        .args(AUTHOR)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Makes a bare repository for `name`, an `owner/name`, under `remotes` where GitHub keeps it:
/// `owner/name.git`. It has one commit on `main` and the tag `v3.15.0a0`. The working copy it came from
/// stays at `work/owner/name`, for [`pull_request`] to commit in.
pub fn remote(remotes: &Path, name: &str) {
    let work = work(remotes, name);
    fs::create_dir_all(&work).unwrap();
    let _ = git(&work, &["init", "-q", "-b", "main"]);
    fs::write(
        work.join("README.rst"),
        "This is Python version 3.15.0 alpha 0\n",
    )
    .unwrap();
    let _ = git(&work, &["add", "README.rst"]);
    let _ = git(&work, &["commit", "-qm", "One"]);
    let _ = git(&work, &["tag", "v3.15.0a0"]);
    let bare = bare(remotes, name);
    let _ = git(
        remotes,
        &[
            "clone",
            "-q",
            "--bare",
            &work.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
    );
}

/// Puts a commit of PR `number`'s own on the [`remote`] for `name`, under `refs/pull/N/head` as GitHub
/// keeps it, on no branch. Gives the commit.
pub fn pull_request(remotes: &Path, name: &str, number: u32) -> String {
    let work = work(remotes, name);
    fs::write(work.join("NEWS"), format!("PR {number}\n")).unwrap();
    let _ = git(&work, &["add", "NEWS"]);
    let _ = git(&work, &["commit", "-qm", &format!("PR {number}")]);
    let commit = git(&work, &["rev-parse", "HEAD"]).trim().to_owned();
    let _ = git(
        &work,
        &[
            "push",
            "-q",
            &bare(remotes, name).to_string_lossy(),
            &format!("HEAD:refs/pull/{number}/head"),
        ],
    );
    let _ = git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
    commit
}

/// Where `remotes` are, as a `file://` URL that ends in a slash. git reports progress over it as it does
/// from GitHub.
pub fn url(remotes: &Path) -> String {
    let path = remotes.to_string_lossy().replace('\\', "/");
    let slash = if path.starts_with('/') { "" } else { "/" };
    format!("file://{slash}{path}/")
}

/// A path for this test run under the repo's ignored `.temp` folder: `.temp/<name>-<pid>`. `.temp` exists
/// by the time this returns; the path itself doesn't. Windows reuses process IDs, so a later run can get
/// the path a failed run left: this removes what it finds there.
pub fn temp_path(name: &str) -> TempPath {
    let temp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp");
    fs::create_dir_all(&temp).unwrap();
    let path = temp.join(format!("{name}-{}", std::process::id()));
    remove(&path);
    TempPath { path }
}

/// A test's folder or file from [`temp_path`], removed when the test is done with it. A test that fails
/// leaves it for reading and names its path. Hold it for as long as the test uses the path: on Windows a
/// folder can't go while a process holds a file in it, so it drops after a git, a language server or an
/// open store that works there, or the test stops them first. It waits a while for one that is ending.
#[derive(Debug)]
pub struct TempPath {
    path: PathBuf,
}

impl std::ops::Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempPath {
    #[expect(
        clippy::print_stderr,
        reason = "libtest shows what a failed test printed, beside its panic"
    )]
    fn drop(&mut self) {
        if std::thread::panicking() {
            // The panic said why first: a failed assertion, or a folder that wouldn't go.
            eprintln!(
                "The test panicked, so it leaves {} for reading.",
                self.path.display()
            );
        } else {
            remove(&self.path);
        }
    }
}

/// How long [`remove`] waits for a process that holds a file in the folder to end. A language server the
/// test asked to stop gets 5 seconds to agree and 5 more to exit before the core kills it, 11 seconds in
/// all (`ENDED_WITHIN` in `tsugiki_core::definitions`), and one the test dropped is killed at once. This
/// covers both, with time to spare for a slow machine.
const HELD_FOR: Duration = Duration::from_secs(15);
/// How long [`remove`] waits between tries.
const TRY_AGAIN: Duration = Duration::from_millis(100);
/// The errors Windows gives for a file that a process holds, `ERROR_SHARING_VIOLATION`, or that it deletes
/// only once the last process lets go of it, `ERROR_ACCESS_DENIED`. Other systems remove a folder whatever
/// works in it.
#[cfg(windows)]
const HELD: [i32; 2] = [32, 5];
#[cfg(not(windows))]
const HELD: [i32; 0] = [];

/// Removes the folder or file at `path`, if there is one. On Windows a process that holds a file in it
/// keeps it there, so this tries again for [`HELD_FOR`] while that process ends. Any other error, or a
/// folder still held, fails the test.
fn remove(path: &Path) {
    let give_up = Instant::now() + HELD_FOR;
    // No line here runs only on a second try, which macOS never makes.
    let pauses = iter::once(Duration::ZERO).chain(iter::repeat(TRY_AGAIN));
    let (removed, held) = pauses
        .map(|pause| {
            thread::sleep(pause);
            let removed = remove_once(path);
            let code = removed.as_ref().err().and_then(io::Error::raw_os_error);
            let held = matches!(code, Some(code) if HELD.contains(&code));
            (removed, held)
        })
        .find(|(_, held)| !held || Instant::now() >= give_up)
        .unwrap();
    let still_held = format!(
        ", which a process that works in it, such as a git, a language server or an open store, still \
         held after {} seconds",
        HELD_FOR.as_secs()
    );
    let why = if held { still_held } else { String::new() };
    let path = path.display();
    assert!(removed.is_ok(), "can't remove {path}{why}: {removed:?}");
}

/// Removes the folder or file at `path`. Nothing there counts as removed.
fn remove_once(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(found) if found.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn work(remotes: &Path, name: &str) -> PathBuf {
    remotes.join("work").join(name)
}

fn bare(remotes: &Path, name: &str) -> PathBuf {
    remotes.join(format!("{name}.git"))
}
