//! The test helpers that several crates share and that use no crate's types: git repositories built with
//! the real git (ADR 0009), the folders under `.temp` the tests work in, the language servers they start,
//! the repo's files they read, and a wait for what another thread does. A change to how a test runs git or
//! where it keeps its files is then made once. A helper that needs a crate's types lives in that crate's
//! `testing` module instead, which other crates' tests reach through its `testing` feature.

#![expect(
    clippy::disallowed_types,
    reason = "tests build their repositories with the real git (ADR 0009)"
)]
#![expect(clippy::unwrap_used, reason = "a failed setup step ends the test")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, fs, io, iter, thread};

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
    let commit = commit(&work, &format!("PR {number}"));
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

/// Commits all that changed in the working copy at `dir`, or nothing if nothing did, with `message`, and
/// gives the commit.
pub fn commit(dir: &Path, message: &str) -> String {
    let _ = git(dir, &["add", "-A"]);
    let _ = git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// Makes a repository at `work` whose `main` holds the files `base`, each a path and its text, and whose
/// branch `pr` writes the files `head` on top of them, deleting each whose text is empty. Clones it bare to
/// `mirror`, as a Mirror holds a PR, and gives the commit at the end of `pr`.
pub fn mirror_with_branch(
    work: &Path,
    mirror: &Path,
    base: &[(&str, &str)],
    head: &[(&str, &str)],
) -> String {
    fs::create_dir_all(work).unwrap();
    let _ = git(work, &["init", "-q", "-b", "main"]);
    write_files(work, base);
    let _ = commit(work, "Base");
    let _ = git(work, &["switch", "-qc", "pr"]);
    write_files(work, head);
    let pr = commit(work, "PR");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    let (from, to) = (work.to_string_lossy(), mirror.to_string_lossy());
    let _ = git(work, &["clone", "-q", "--bare", &from, &to]);
    pr
}

/// Writes each of `files`, a path under `dir` and its text, or deletes the file where the text is empty.
fn write_files(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = dir.join(path);
        if text.is_empty() {
            fs::remove_file(path).unwrap();
        } else {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }
}

/// The ty that `TSUGIKI_TY` names, which `just cargo-test` and `just rust-coverage` set to the one
/// `scripts/test-servers.ts` fetches.
pub fn ty() -> PathBuf {
    test_server("TSUGIKI_TY")
}

/// The rust-analyzer that `TSUGIKI_RUST_ANALYZER` names, which `just cargo-test` and `just rust-coverage`
/// set to the one in the toolchain `rust-toolchain.toml` pins.
pub fn rust_analyzer() -> PathBuf {
    test_server("TSUGIKI_RUST_ANALYZER")
}

/// The clangd that `TSUGIKI_CLANGD` names, which `just cargo-test` and `just rust-coverage` set to the one
/// `scripts/test-servers.ts` fetches.
pub fn clangd() -> PathBuf {
    test_server("TSUGIKI_CLANGD")
}

/// The language server the environment variable `name` names. One that isn't there fails the test.
fn test_server(name: &str) -> PathBuf {
    let path = PathBuf::from(env::var_os(name).unwrap_or_default());
    assert!(path.is_file(), "{name}={path:?}: see just test-servers");
    path
}

/// The top of the repo.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `.rs` files under `dir`, at any depth.
pub fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources
}

/// What [`check_generated`] does with a generated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generated {
    /// Fails unless the file says what was generated.
    Compare,
    /// Writes what was generated to the file.
    Update,
}

/// Checks a file the repo commits against `fresh`, what generates it now: [`check_generated`] on `path`,
/// from the top of the repo, which updates it when the environment variable `env_var` is `1`. A failure
/// says to run `fix`.
pub fn compare_or_update(path: &str, fresh: &str, env_var: &str, fix: &str) {
    use Generated::{Compare, Update};
    let update = env::var(env_var).as_deref() == Ok("1");
    let generated = if update { Update } else { Compare };
    check_generated(&repo_root().join(path), fresh, generated, fix);
}

/// Writes `fresh` to `file`, or fails unless `file` already says the same. Line ends don't count, since git
/// can check a file out with CRLF. A failure says to run `fix` and commit the result.
pub fn check_generated(file: &Path, fresh: &str, generated: Generated, fix: &str) {
    if generated == Generated::Update {
        fs::write(file, fresh).unwrap();
    }
    let committed = fs::read_to_string(file).unwrap_or_default();
    assert!(
        committed.replace("\r\n", "\n") == fresh.replace("\r\n", "\n"),
        "{} is out of date: run `{fix}` and commit the result",
        file.display()
    );
}

/// How long [`within`] waits between checks.
const CHECK_AGAIN: Duration = Duration::from_millis(20);

/// Whether `check` holds within `patience`. It checks again every little while, and gives up on the first
/// check that fails once `patience` is up.
pub fn within(patience: Duration, mut check: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + patience;
    // No line here runs only on a second check, which a fast machine never makes.
    let pauses = iter::once(Duration::ZERO).chain(iter::repeat(CHECK_AGAIN));
    pauses
        .map(|pause| {
            thread::sleep(pause);
            (check(), Instant::now() >= until)
        })
        .find(|(held, late)| *held || *late)
        .unwrap()
        .0
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
    let temp = repo_root().join(".temp");
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
