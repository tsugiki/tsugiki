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
use std::time::Duration;
use std::{fs, io, thread};

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
            eprintln!("The test left {} for reading.", self.path.display());
        } else {
            remove(&self.path);
        }
    }
}

/// How many times [`remove`] tries, [`TRY_AGAIN`] apart. A process the test killed or asked to stop, such as
/// a language server, takes a moment to end, and up to 11 seconds when it doesn't agree to stop.
const TRIES: u32 = 150;
const TRY_AGAIN: Duration = Duration::from_millis(100);

/// Removes the folder or file at `path`, if there is one. On Windows a process that holds a file in it
/// keeps it there, so this tries again while that process ends, and fails the test if it doesn't.
fn remove(path: &Path) {
    let mut removed = Ok(());
    for tried in 0..TRIES {
        thread::sleep(TRY_AGAIN * tried.min(1));
        removed = remove_once(path);
        if removed.is_ok() {
            break;
        }
    }
    assert!(
        removed.is_ok(),
        "can't remove {}: {removed:?}",
        path.display()
    );
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
