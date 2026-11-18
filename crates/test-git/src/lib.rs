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

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, io, iter, thread};

/// The name of the author of each commit a test makes, so a commit needs no git config on the machine.
const NAME: &str = "Contributor";
/// That author's address.
const EMAIL: &str = "contributor@example.com";

/// The author of each commit a test makes, as a `git fast-import` stream names its committer.
pub fn committer() -> String {
    format!("{NAME} <{EMAIL}>")
}

/// The config of each git a test runs: the author, and `maintenance.auto=false`. Without it, each commit
/// would start a `git maintenance run --auto`, about 500 processes in a run of the core's tests, for
/// repositories too small to need it.
fn config() -> [String; 6] {
    [
        "-c".to_owned(),
        format!("user.name={NAME}"),
        "-c".to_owned(),
        format!("user.email={EMAIL}"),
        "-c".to_owned(),
        "maintenance.auto=false".to_owned(),
    ]
}

/// What points git at another repository than the one in the folder it runs in. A hook or an alias that
/// runs the tests can leave these set, and then `reset --hard` would act on the developer's checkout.
const ELSEWHERE: [&str; 3] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"];

/// Runs git in `dir` and gives what it printed. A git that fails ends the test with what it said.
pub fn git(dir: &Path, args: &[&str]) -> String {
    git_reading(dir, args, &[])
}

/// [`git`], with `input` on git's stdin, as `git fast-import` reads a whole history from one stream. git
/// reads all of it before it says much, so this writes it all before it reads what git printed.
pub fn git_reading(dir: &Path, args: &[&str], input: &[u8]) -> String {
    let mut command = Command::new("git");
    for name in ELSEWHERE {
        let _ = command.env_remove(name);
    }
    let mut child = command
        .args(config())
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A git that ends early closes its stdin, and then what it said tells more than the failed write.
    let written = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    written.unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// Makes a bare repository for `name`, an `owner/name`, under `remotes` where GitHub keeps it:
/// `owner/name.git`. It has one commit on `main` and the tag `v3.15.0a0`, and no remote of its own. The
/// working copy it came from stays at `work/owner/name`, for a test to commit in and push from. A test run
/// builds the remote of each `name` once, and later calls copy it.
pub fn remote(remotes: &Path, name: &str) {
    /// The working copy and the bare repository of each `name` built so far.
    static BUILT: Mutex<BTreeMap<String, Arc<(Snapshot, Snapshot)>>> = Mutex::new(BTreeMap::new());
    /// Tells apart the folders of two tests that build the same remote at once.
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    // Two tests that build the same remote at once each build it, rather than one waiting on the other.
    let found = BUILT.lock().unwrap().get(name).cloned();
    let built = found.unwrap_or_else(|| {
        let build = BUILDS.fetch_add(1, Ordering::Relaxed);
        let folder = temp_path(&format!("remote-{build}"));
        let (work, bare) = (work(&folder, name), bare(&folder, name));
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
        let (from, to) = (work.to_string_lossy(), bare.to_string_lossy());
        let _ = git(&folder, &["clone", "-q", "--bare", &from, &to]);
        // A copy would point `origin` at the working copy this folder held.
        let _ = git(&bare, &["remote", "remove", "origin"]);
        let built = Arc::new((Snapshot::of(&work), Snapshot::of(&bare)));
        let mut all = BUILT.lock().unwrap();
        Arc::clone(all.entry(name.to_owned()).or_insert(built))
    });
    built.0.write(&work(remotes, name));
    built.1.write(&bare(remotes, name));
}

/// Puts a commit of PR `number`'s own on the [`remote`] for `name`, under `refs/pull/N/head` as GitHub
/// keeps it, on no branch: the remote's `main` with a file `NEWS` added. Gives the commit. One `git
/// fast-import` writes it, where a commit in the working copy and a push would start nine gits.
pub fn pull_request(remotes: &Path, name: &str, number: u32) -> String {
    let (message, news) = (format!("PR {number}"), format!("PR {number}\n"));
    let stream = format!(
        "commit refs/pull/{number}/head\nmark :1\ncommitter {} now\ndata {}\n{message}\n\
         from refs/heads/main\nM 100644 inline NEWS\ndata {}\n{news}\nget-mark :1\n",
        committer(),
        message.len(),
        news.len(),
    );
    let args = ["fast-import", "--quiet", "--date-format=now"];
    git_reading(&bare(remotes, name), &args, stream.as_bytes())
        .trim()
        .to_owned()
}

/// Commits all that changed in the working copy at `dir`, or nothing if nothing did, with `message`, and
/// gives the commit.
pub fn commit(dir: &Path, message: &str) -> String {
    let _ = git(dir, &["add", "-A"]);
    let _ = git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// Makes a bare repository at `mirror`, as a Mirror holds a PR, whose `main` holds the files `base`, each a
/// path and its text, and whose branch `pr` writes the files `head` on top of them, deleting each whose text
/// is empty. Gives the commit at the end of `pr`. The repository it clones from is built at `work` and gone
/// by the time this returns, and the mirror has no remote. A test run builds the mirror of the same files
/// once, and later calls copy it.
pub fn mirror_with_branch(
    work: &Path,
    mirror: &Path,
    base: &[(&str, &str)],
    head: &[(&str, &str)],
) -> String {
    static BUILT: Mutex<BTreeMap<BuiltFrom, Arc<Built>>> = Mutex::new(BTreeMap::new());
    let owned = |files: &[(&str, &str)]| -> Vec<(String, String)> {
        files
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect()
    };
    let key = BuiltFrom {
        base: owned(base),
        head: owned(head),
    };
    // Two tests that build the same mirror at once each build it, rather than one waiting on the other.
    let found = BUILT.lock().unwrap().get(&key).cloned();
    if let Some(built) = found {
        built.mirror.write(mirror);
        return built.pr.clone();
    }
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
    // A copy would point `origin` at the folder of the test that built it, which may be gone.
    let _ = git(mirror, &["remote", "remove", "origin"]);
    remove(work);
    let built = Built {
        mirror: Snapshot::of(mirror),
        pr: pr.clone(),
    };
    let _ = BUILT.lock().unwrap().insert(key, Arc::new(built));
    pr
}

/// The files a [`mirror_with_branch`] call gave, each a path and its text.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct BuiltFrom {
    base: Vec<(String, String)>,
    head: Vec<(String, String)>,
}

/// A mirror [`mirror_with_branch`] built, and the commit at the end of its PR.
struct Built {
    mirror: Snapshot,
    pr: String,
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

/// The fake language server in `tsugiki-lsp` that `TSUGIKI_FAKE_SERVER` names, which `just cargo-test` and
/// `just rust-coverage` build and set.
pub fn fake_server() -> PathBuf {
    test_server("TSUGIKI_FAKE_SERVER")
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

/// The Rust item `item` without the `pub` or `pub(...)` in front of it.
pub fn without_visibility(item: &str) -> &str {
    let item = match item.strip_prefix("pub") {
        Some(after) if after.starts_with('(') => after.split_once(')').map_or("", |(_, rest)| rest),
        Some(after) => after,
        None => item,
    };
    item.trim_start()
}

/// A file the repo commits that code generates, and how a developer brings it up to date.
#[derive(Debug, Clone, Copy)]
pub struct GeneratedFile<'a> {
    /// Where it is, from the top of the repo, unless the path is absolute.
    pub path: &'a str,
    /// The environment variable that, set to `1`, has [`compare_or_update`] write the file.
    pub update_with: &'a str,
    /// The command that sets [`GeneratedFile::update_with`] and runs the check.
    pub fix: &'a str,
}

/// What [`check_generated`] does with a generated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generated {
    /// Fails unless the file says what was generated.
    Compare,
    /// Writes what was generated to the file.
    Update,
}

/// Checks `file` against `fresh`, what generates it now, with [`check_generated`], which updates it when the
/// environment variable [`GeneratedFile::update_with`] is `1`.
pub fn compare_or_update(file: &GeneratedFile<'_>, fresh: &str) {
    use Generated::{Compare, Update};
    let update = env::var(file.update_with).as_deref() == Ok("1");
    let generated = if update { Update } else { Compare };
    check_generated(file, fresh, generated);
}

/// Writes `fresh` to `file`, or fails unless `file` already says the same. Line ends don't count, since git
/// can check a file out with CRLF. A failure says to run [`GeneratedFile::fix`] and commit the result.
pub fn check_generated(file: &GeneratedFile<'_>, fresh: &str, generated: Generated) {
    let path = repo_root().join(file.path);
    if generated == Generated::Update {
        fs::write(&path, fresh).unwrap();
    }
    let committed = fs::read_to_string(&path);
    let lines = |text: &str| text.replace("\r\n", "\n");
    let same = committed
        .as_deref()
        .is_ok_and(|text| lines(text) == lines(fresh));
    let unread = (committed.err())
        .map(|cause| format!(" (it can't be read: {cause})"))
        .unwrap_or_default();
    assert!(
        same,
        "{} is out of date{unread}: run `{}` and commit the result",
        path.display(),
        file.fix
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

/// What a folder holds, read into memory: each folder in it, empty ones included, and each file with its
/// bytes. A fixture that many tests share builds once with git, and each test writes its own copy from this
/// without starting a git. On Windows a git costs about 30 ms to start.
pub struct Snapshot {
    /// Paths relative to the folder read, each folder before what it holds.
    entries: Vec<(PathBuf, Entry)>,
}

enum Entry {
    Folder,
    File(Vec<u8>),
}

impl Snapshot {
    /// Reads what `folder` holds.
    pub fn of(folder: &Path) -> Self {
        let mut entries = Vec::new();
        let mut folders = vec![PathBuf::new()];
        while let Some(relative) = folders.pop() {
            for entry in fs::read_dir(folder.join(&relative)).unwrap() {
                let entry = entry.unwrap();
                let path = relative.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    entries.push((path.clone(), Entry::Folder));
                    folders.push(path);
                } else {
                    entries.push((path, Entry::File(fs::read(entry.path()).unwrap())));
                }
            }
        }
        Self { entries }
    }

    /// Writes what the folder held into `folder`, which this makes.
    pub fn write(&self, folder: &Path) {
        fs::create_dir_all(folder).unwrap();
        for (path, entry) in &self.entries {
            match entry {
                Entry::Folder => fs::create_dir_all(folder.join(path)).unwrap(),
                Entry::File(bytes) => fs::write(folder.join(path), bytes).unwrap(),
            }
        }
    }
}

fn work(remotes: &Path, name: &str) -> PathBuf {
    remotes.join("work").join(name)
}

fn bare(remotes: &Path, name: &str) -> PathBuf {
    remotes.join(format!("{name}.git"))
}
