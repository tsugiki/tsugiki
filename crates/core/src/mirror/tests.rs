use std::fs;
use std::io::{self, Read, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use super::*;
use crate::error::ErrorKind;
use crate::github::pull_requests::PrHead;
use crate::github::{CommitSha, Login};
use crate::status::Status;

/// Longer than any of these tests should wait on a working git.
const PLENTY: Duration = Duration::from_secs(60);

const RECORDED: &str = include_str!("../../tests/fixtures/git/clone-progress.txt");

/// Each Repository whose heads arrived, by name, with the PRs whose heads they were.
type Arrivals = Receiver<(String, Vec<u32>)>;

/// Each Repository whose fetch of heads failed, by name, with the PRs whose heads they were and why.
type Failures = Receiver<(String, Vec<u32>, String)>;

/// Announcements that pass on each clone, and nothing of fetches.
fn cloning_only(announcing: Sender<Option<Cloning>>) -> Announce {
    Announce::cloning_only(Box::new(move |cloning| {
        drop(announcing.send(cloning.cloned()));
    }))
}

fn named(name: &str) -> RepositoryName {
    RepositoryName::parse(name).unwrap()
}

/// The answer of a user who has the mirror download everything.
fn download() -> Option<MirrorSource> {
    Some(MirrorSource::Download)
}

#[expect(
    clippy::disallowed_types,
    reason = "tests build their repositories with the real git (ADR 0009)"
)]
fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A test's folders: Tsugiki's data, and the remotes it clones from.
struct Place {
    folders: Folders,
    remotes: PathBuf,
}

impl Place {
    fn new(name: &str) -> Self {
        let root = crate::testing::temp_path(&format!("mirror-{name}"));
        let data = root.join("data");
        let remotes = root.join("remotes");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&remotes).unwrap();
        Self {
            folders: Folders {
                data: data.clone(),
                settings: data,
            },
            remotes,
        }
    }

    /// A bare repository for `name` among the remotes, with one commit and a tag.
    fn remote(&self, name: &RepositoryName) {
        let work = self.remotes.join(format!("work-{}", name.name()));
        fs::create_dir_all(&work).unwrap();
        let _ = git(&work, &["init", "-q"]);
        fs::write(
            work.join("README.rst"),
            "This is Python version 3.15.0 alpha 0\n",
        )
        .unwrap();
        let _ = git(&work, &["add", "README.rst"]);
        let _ = git(
            &work,
            &[
                "-c",
                "user.name=Contributor",
                "-c",
                "user.email=contributor@example.com",
                "commit",
                "-qm",
                "One",
            ],
        );
        let _ = git(&work, &["tag", "v3.15.0a0"]);
        let bare = self
            .remotes
            .join(name.owner())
            .join(format!("{}.git", name.name()));
        let _ = git(
            &self.remotes,
            &[
                "clone",
                "-q",
                "--bare",
                &work.to_string_lossy(),
                &bare.to_string_lossy(),
            ],
        );
    }

    /// Where the remotes are, as a `file://` URL, over which git reports progress as it does from GitHub.
    fn clone_source(&self) -> Remotes {
        let path = self.remotes.to_string_lossy().replace('\\', "/");
        let slash = if path.starts_with('/') { "" } else { "/" };
        Remotes::under(format!("file://{slash}{path}/"))
    }

    fn start(&self) -> (Mirrors, Receiver<Option<Cloning>>) {
        let (announcing, announced) = channel();
        let mirrors = Mirrors::start(
            Ok(self.folders.clone()),
            self.clone_source(),
            cloning_only(announcing),
        );
        (mirrors, announced)
    }
}

impl Place {
    /// A PR `number` on the remote for `name`, as GitHub keeps one: a commit of its own under
    /// `refs/pull/N/head`, on no branch. Gives the commit.
    fn pull_request(&self, name: &RepositoryName, number: u32) -> String {
        let work = self.remotes.join(format!("work-{}", name.name()));
        fs::write(work.join("NEWS"), format!("PR {number}\n")).unwrap();
        let _ = git(&work, &["add", "NEWS"]);
        let _ = git(
            &work,
            &[
                "-c",
                "user.name=Contributor",
                "-c",
                "user.email=contributor@example.com",
                "commit",
                "-qm",
                &format!("PR {number}"),
            ],
        );
        let commit = git(&work, &["rev-parse", "HEAD"]).trim().to_owned();
        let bare = self
            .remotes
            .join(name.owner())
            .join(format!("{}.git", name.name()));
        let _ = git(
            &work,
            &[
                "push",
                "-q",
                &bare.to_string_lossy(),
                &format!("HEAD:refs/pull/{number}/head"),
            ],
        );
        let _ = git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
        commit
    }
}

/// What the worker announced for one clone, from its start until it ended.
fn one_clone(announced: &Receiver<Option<Cloning>>) -> Vec<Cloning> {
    let mut clone = Vec::new();
    while let Some(cloning) = announced.recv_timeout(PLENTY).unwrap() {
        clone.push(cloning);
    }
    clone
}

#[test]
fn adding_a_repository_clones_its_mirror_and_shows_the_clone_until_it_ends() {
    let place = Place::new("clones");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(hello.clone(), download());
    let clone = one_clone(&announced);
    mirrors.stop(PLENTY);

    assert_eq!(
        clone.first(),
        Some(&Cloning {
            repository: hello.clone(),
            progress: CloneProgress::Starting,
        })
    );
    assert!(clone.contains(&Cloning {
        repository: hello.clone(),
        progress: CloneProgress::Receiving { percent: 100 },
    }));
    let mirror = place.folders.mirror(&hello);
    assert_eq!(
        git(&mirror, &["rev-parse", "--is-bare-repository"]),
        "true\n"
    );
    assert_eq!(git(&mirror, &["log", "--format=%s"]), "One\n");
    assert_eq!(
        git(&mirror, &["tag"]),
        "",
        "a mirror fetches no tags (ADR 0002)"
    );
    assert!(!place.folders.repository(&hello).join(PARTIAL).exists());
}

#[test]
fn a_mirror_sits_in_its_repositorys_folder() {
    let folders = Folders {
        data: PathBuf::from("data"),
        settings: PathBuf::from("settings"),
    };

    assert_eq!(
        folders.mirror(&named("Python/CPython")),
        Path::new("data")
            .join("repos")
            .join("python__cpython")
            .join("mirror.git")
    );
}

#[test]
fn quitting_stops_a_clone_under_way() {
    let place = Place::new("quits");
    // A server that takes git's request and never answers, so the clone would wait forever.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let (announcing, announced) = channel();
    let mirrors = Mirrors::start(
        Ok(place.folders.clone()),
        Remotes::under(format!("http://127.0.0.1:{}/", address.port())),
        cloning_only(announcing),
    );
    let hello = named("octo-org/hello");

    mirrors.requests().clone_mirror(hello.clone(), download());
    let (mut waiting_git, _) = server.accept().unwrap();
    waiting_git
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        waiting_git.read_exact(&mut byte).unwrap();
        request.extend(byte);
    }
    assert!(announced.recv_timeout(PLENTY).unwrap().is_some());
    let quitting = Instant::now();
    mirrors.stop(PLENTY);
    let quit_in = quitting.elapsed();
    // git hands the download to a helper of its own, which has to end with it.
    let after_quitting = waiting_git.read(&mut [0; 64]);

    assert!(quit_in < Duration::from_secs(10), "{quit_in:?}");
    assert!(
        matches!(after_quitting, Ok(0) | Err(_))
            && !matches!(&after_quitting, Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock),
        "git's helper still holds the connection: {after_quitting:?}"
    );
    assert!(!place.folders.mirror(&hello).exists());
}

#[test]
fn the_open_repositorys_mirror_goes_ahead_of_those_waiting() {
    let place = Place::new("order");
    let [first, second, open] = [
        named("octo-org/first"),
        named("octo-org/second"),
        named("octo-org/open"),
    ];
    for name in [&first, &second, &open] {
        place.remote(name);
    }
    let (mirrors, announced) = place.start();
    let requests = mirrors.requests();

    requests.clone_mirror(first.clone(), download());
    requests.clone_mirror(second.clone(), download());
    requests.clone_mirror(second.clone(), download());
    requests.clone_mirror_next(first.clone(), download());
    requests.clone_mirror(open.clone(), download());
    requests.clone_mirror_next(open.clone(), download());
    let order: Vec<RepositoryName> = (0..3)
        .map(|_| one_clone(&announced).remove(0).repository)
        .collect();
    mirrors.stop(PLENTY);

    assert_eq!(order, [first, open, second]);
}

#[test]
fn a_repository_whose_mirror_is_there_isnt_cloned_again() {
    let place = Place::new("cloned");
    let [cloned, next] = [named("octo-org/cloned"), named("octo-org/next")];
    place.remote(&next);
    fs::create_dir_all(place.folders.mirror(&cloned)).unwrap();
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(cloned, download());
    mirrors
        .requests()
        .clone_mirror_next(next.clone(), download());
    let clone = one_clone(&announced);
    mirrors.stop(PLENTY);

    assert!(clone.iter().all(|cloning| cloning.repository == next));
}

#[test]
fn a_failed_clone_leaves_no_folder_and_the_next_one_goes_on() {
    let place = Place::new("fails");
    let [missing, next] = [named("octo-org/missing"), named("octo-org/next")];
    place.remote(&next);
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(missing.clone(), download());
    mirrors.requests().clone_mirror(next.clone(), download());
    let failed = one_clone(&announced);
    let _ = one_clone(&announced);
    mirrors.stop(PLENTY);

    assert!(failed.iter().all(|cloning| cloning.repository == missing));
    let partial = place.folders.repository(&missing).join(PARTIAL);
    assert!(!partial.exists());
    assert!(!place.folders.mirror(&missing).exists());
    assert!(place.folders.mirror(&next).exists());
}

#[test]
fn a_clone_that_cant_start_leaves_only_a_trace() {
    let place = Place::new("cant-start");
    // A file where the Repositories' folders go, so no folder for a clone can be made.
    fs::write(place.folders.data.join("repos"), "not a folder").unwrap();
    let (mirrors, announced) = place.start();

    mirrors
        .requests()
        .clone_mirror(named("octo-org/hello"), download());
    mirrors.stop(PLENTY);

    assert!(announced.try_recv().is_err());
}

#[test]
fn a_leftover_clone_that_cant_be_deleted_leaves_only_a_trace() {
    let place = Place::new("cant-delete");
    let hello = named("octo-org/hello");
    let partial = place.folders.repository(&hello).join(PARTIAL);
    fs::create_dir_all(partial.parent().unwrap()).unwrap();
    fs::write(&partial, "a file, which deleting a folder can't remove").unwrap();
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(hello.clone(), download());
    mirrors.stop(PLENTY);

    assert!(announced.try_recv().is_err());
    assert!(!place.folders.mirror(&hello).exists());
}

#[test]
fn asking_after_quitting_only_leaves_a_trace() {
    let place = Place::new("after-quitting");
    let (mirrors, announced) = place.start();
    let requests = mirrors.requests();

    mirrors.stop(PLENTY);
    requests.clone_mirror(named("octo-org/hello"), download());

    assert!(announced.try_recv().is_err());
}

#[test]
fn without_its_data_folder_tsugiki_clones_nothing() {
    let (announcing, announced) = channel();
    let mirrors = Mirrors::start(
        Err(Error::new(ErrorKind::Internal, "There is no data folder.")),
        Remotes::github(),
        cloning_only(announcing),
    );

    mirrors
        .requests()
        .clone_mirror(named("python/cpython"), download());
    mirrors.stop(PLENTY);

    assert!(announced.try_recv().is_err());
}

#[test]
fn a_recorded_clone_reads_as_receiving_then_resolving() {
    let read: Vec<CloneProgress> = RECORDED
        .split(['\r', '\n'])
        .filter_map(read_progress)
        .collect();

    let receiving: Vec<u8> = read
        .iter()
        .filter_map(|progress| match progress {
            CloneProgress::Receiving { percent } => Some(*percent),
            CloneProgress::Starting | CloneProgress::Resolving { .. } => None,
        })
        .collect();
    let resolving: Vec<u8> = read
        .iter()
        .filter_map(|progress| match progress {
            CloneProgress::Resolving { percent } => Some(*percent),
            CloneProgress::Starting | CloneProgress::Receiving { .. } => None,
        })
        .collect();
    assert_eq!(receiving, [0, 1, 38, 42, 78, 99, 100, 100]);
    assert_eq!(resolving, [0, 1, 38, 42, 78, 99, 100, 100]);
    assert_eq!(read.first(), Some(&CloneProgress::Receiving { percent: 0 }));
    assert_eq!(
        read.last(),
        Some(&CloneProgress::Resolving { percent: 100 })
    );
}

#[test]
fn lines_from_the_server_or_without_a_percentage_say_nothing() {
    for line in [
        "Cloning into bare repository 'big.git'...",
        "remote: Counting objects:  42% (1032/2455)",
        "remote: Total 2455 (delta 1514), reused 0 (delta 0), pack-reused 0 (from 0)",
        "Receiving objects: lots",
        "Receiving objects: 420% (1/2)",
        "Counting objects:  42% (1032/2455)",
        "",
    ] {
        assert_eq!(read_progress(line), None, "{line}");
    }
}

/// A git that tells the test each time the worker starts it, and ends when the test says.
struct FakeGit {
    starts: Sender<FakeStart>,
    /// Handed to each start, for the test to say where the refs landed.
    landed: Sender<String>,
    /// What each read of the mirror's refs gives, which a fetch that succeeds sends first.
    refs: Receiver<String>,
    /// The answer to each `git --version`, or nothing for a git that isn't there.
    versions: Receiver<Option<String>>,
}

/// One start of the fake git.
struct FakeStart {
    args: Vec<String>,
    input: String,
    /// Closing it closes git's error output, as git does when it ends.
    output: io::PipeWriter,
    outcome: Sender<bool>,
    /// Hears when the worker has asked how git ended, so what the test sends next comes after.
    asked: Receiver<()>,
    /// Hears when the worker stops this git.
    stopped: Receiver<()>,
    /// What the worker reads back from the mirror's refs after this git.
    refs: Sender<String>,
}

impl FakeStart {
    /// Ends this git, and waits for the worker to hear how.
    fn end(self, outcome: Outcome) {
        self.outcome.send(outcome == Outcome::Succeeded).unwrap();
        drop(self.output);
        self.asked.recv_timeout(PLENTY).unwrap();
    }

    /// Ends this git. A fetch leaves each head it asked for at the commit `head` gives it.
    fn succeed(self) {
        if self.args.iter().any(|arg| arg == "fetch") {
            let landed = self.numbers().into_iter().map(head).collect();
            self.succeed_with(landed);
        } else {
            self.end(Outcome::Succeeded);
        }
    }

    /// Ends this git, with the mirror's PR refs at `landed` after it.
    fn succeed_with(self, landed: Vec<PrHead>) {
        let refs: String = landed
            .iter()
            .map(|PrHead { number, commit, .. }| {
                format!("{} refs/tsugiki/pull/{number}/head\n", commit.as_str())
            })
            .collect();
        self.refs.send(refs).unwrap();
        self.end(Outcome::Succeeded);
    }

    /// Ends this git, and leaves the mirror's refs unreadable.
    fn succeed_unread(self) {
        self.end(Outcome::Succeeded);
    }

    /// The PRs whose heads this fetch asks for.
    fn numbers(&self) -> Vec<u32> {
        self.refspecs()
            .iter()
            .filter_map(|refspec| {
                refspec
                    .strip_prefix("+refs/pull/")?
                    .split('/')
                    .next()?
                    .parse()
                    .ok()
            })
            .collect()
    }

    fn fail(self) {
        self.end(Outcome::Failed);
    }

    /// The refspecs of the PR heads this fetch asks for.
    fn refspecs(&self) -> Vec<&str> {
        let heads = self.input.lines();
        heads
            .filter(|line| line.starts_with("+refs/pull/"))
            .collect()
    }

    /// The refspecs of the branches this fetch brings up to date.
    fn branches(&self) -> Vec<&str> {
        let branches = self.input.lines();
        branches
            .filter(|line| line.starts_with("+refs/heads/"))
            .collect()
    }
}

/// Ends `start` as git does when it fails, with these lines of error output.
fn fail_saying(mut start: FakeStart, lines: &str) {
    start.output.write_all(lines.as_bytes()).unwrap();
    start.fail();
}

#[derive(PartialEq, Eq)]
enum Outcome {
    Succeeded,
    Failed,
}

struct FakeRun {
    outcome: Receiver<bool>,
    asking: Sender<()>,
    stopping: Sender<()>,
}

impl Run for FakeRun {
    fn succeeded(&mut self) -> bool {
        let succeeded = self.outcome.recv().unwrap_or(false);
        self.asking.send(()).unwrap();
        succeeded
    }

    fn stop(&mut self) {
        self.stopping.send(()).unwrap();
    }
}

impl Git for FakeGit {
    fn start(&self, args: &[&OsStr], input: &[u8]) -> io::Result<Started> {
        let (output, writer) = io::pipe()?;
        let (outcome, outcomes) = channel();
        let (stopping, stopped) = channel();
        let (asking, asked) = channel();
        // A test that has stopped listening has a git that can't start.
        self.starts
            .send(FakeStart {
                args: args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
                input: String::from_utf8_lossy(input).into_owned(),
                output: writer,
                outcome,
                asked,
                stopped,
                refs: self.landed.clone(),
            })
            .map_err(|_unheard| io::Error::other("the test has stopped listening"))?;
        Ok(Started {
            run: Box::new(FakeRun {
                outcome: outcomes,
                asking,
                stopping,
            }),
            output: Box::new(output),
        })
    }

    fn read(&self, args: &[&OsStr]) -> io::Result<String> {
        if args == [OsStr::new("--version")] {
            return match self.versions.try_recv() {
                Ok(Some(version)) => Ok(version),
                Ok(None) => Err(io::Error::from(io::ErrorKind::NotFound)),
                Err(_none) => Ok("git version 2.43.0.windows.1".to_owned()),
            };
        }
        // A look at a clone the user has goes to the real git, which reads it as the worker would.
        let looks = [OsStr::new("rev-parse"), OsStr::new("config")];
        if args.get(2).is_some_and(|command| looks.contains(command)) {
            return live::read_git(args);
        }
        self.refs
            .try_recv()
            .map_err(|_none| io::Error::other("the test gave no refs"))
    }
}

impl Place {
    /// The worker with the fake git, and each start of it. A head that is behind waits longer than a test.
    fn start_faked(&self) -> (Mirrors, Receiver<FakeStart>) {
        self.start_faked_with(PLENTY)
    }

    /// The worker with the fake git, whose heads that are behind wait `patience` to be fetched once more.
    fn start_faked_with(&self, patience: Duration) -> (Mirrors, Receiver<FakeStart>) {
        self.start_faked_announcing(patience, Announce::cloning_only(Box::new(|_| {})))
    }

    /// The worker with the fake git, each start of it, and each Repository's heads that arrived.
    fn start_faked_hearing(&self, patience: Duration) -> (Mirrors, Receiver<FakeStart>, Arrivals) {
        let (arriving, arrivals) = channel();
        let announce = Announce {
            fetched: Box::new(move |repository, numbers| {
                drop(arriving.send((repository.to_string(), numbers.to_vec())));
            }),
            ..Announce::cloning_only(Box::new(|_| {}))
        };
        let (mirrors, starts) = self.start_faked_announcing(patience, announce);
        (mirrors, starts, arrivals)
    }

    /// The worker with the fake git, each start of it, and each Repository's heads whose fetch failed, with
    /// why.
    fn start_faked_hearing_failures(&self) -> (Mirrors, Receiver<FakeStart>, Failures) {
        let (failing, failures) = channel();
        let announce = Announce {
            failed: Box::new(move |repository, numbers, why| {
                drop(failing.send((repository.to_string(), numbers.to_vec(), why.to_owned())));
            }),
            ..Announce::cloning_only(Box::new(|_| {}))
        };
        let (mirrors, starts) = self.start_faked_announcing(PLENTY, announce);
        (mirrors, starts, failures)
    }

    /// The worker with the fake git, which tells `announce` as it goes, and each start of it.
    fn start_faked_announcing(
        &self,
        patience: Duration,
        announce: Announce,
    ) -> (Mirrors, Receiver<FakeStart>) {
        let (_versions, answers) = channel();
        self.start_faked_from(self.clone_source().patient(patience), announce, answers)
    }

    /// The worker with the fake git, cloning from `remotes`, and each start of it. Each `git --version`
    /// takes the next of `versions`, where nothing is a git that isn't there, or git 2.43 once they run out.
    fn start_faked_from(
        &self,
        remotes: Remotes,
        announce: Announce,
        versions: Receiver<Option<String>>,
    ) -> (Mirrors, Receiver<FakeStart>) {
        let (starting, starts) = channel();
        let (landed, refs) = channel();
        let mirrors = Mirrors::with_git(
            Box::new(FakeGit {
                starts: starting,
                landed,
                refs,
                versions,
            }),
            Ok(self.folders.clone()),
            remotes,
            announce,
        );
        (mirrors, starts)
    }

    /// A mirror that is already there, as far as the worker can tell.
    fn mirrored(&self, repository: &RepositoryName) {
        fs::create_dir_all(self.folders.mirror(repository)).unwrap();
    }
}

fn head(number: u32) -> PrHead {
    PrHead {
        number,
        commit: CommitSha::parse(&format!("{number:040x}")).unwrap(),
        target_branch: "main".to_owned(),
    }
}

/// PR `number`'s head, at the commit `head(9)` gives: what its ref held before GitHub moved it.
fn older(number: u32) -> PrHead {
    PrHead {
        commit: head(9).commit,
        ..head(number)
    }
}

fn refspec(number: u32) -> String {
    format!("+refs/pull/{number}/head*:refs/tsugiki/pull/{number}/head*")
}

#[test]
fn the_heads_handed_over_together_go_in_one_fetch_into_the_mirror() {
    let place = Place::new("one-batch");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();

    mirrors
        .requests()
        .fetch_heads(cpython.clone(), vec![head(2), head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(fetch.refspecs(), [refspec(1), refspec(2)]);
    let mirror = place.folders.mirror(&cpython);
    assert_eq!(
        fetch.args,
        [
            "-c",
            "maintenance.auto=false",
            "-c",
            "http.lowSpeedLimit=1",
            "-c",
            "http.lowSpeedTime=60",
            "-C",
            &mirror.to_string_lossy(),
            "fetch",
            "--stdin",
            "--no-tags",
            "--no-write-fetch-head",
            "--prune",
            "origin"
        ]
    );
    fetch.succeed();
    mirrors.stop(PLENTY);
    assert!(starts.recv().is_err(), "one fetch, and nothing after it");
}

#[test]
fn heads_handed_over_during_a_fetch_go_in_the_next() {
    let place = Place::new("next-batch");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let first = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    requests.fetch_heads(cpython.clone(), vec![head(3)]);
    first.succeed();
    let next = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(next.refspecs(), [refspec(2), refspec(3)]);
    next.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_fetched_head_isnt_fetched_again_and_a_failed_one_is() {
    let place = Place::new("fetched-once");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().succeed();
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    let second = starts.recv_timeout(PLENTY).unwrap();
    assert_eq!(second.refspecs(), [refspec(2)]);
    second.fail();
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    let third = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(third.refspecs(), [refspec(2)]);
    third.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_failed_fetch_tries_again_when_github_is_back() {
    let place = Place::new("fetch-again");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().fail();
    requests.follow(&Status::Offline);
    requests.follow(&Status::Online {
        login: Login::new("octocat"),
    });
    let again = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(again.refspecs(), [refspec(1)]);
    assert_eq!(again.branches(), ["+refs/heads/main*:refs/heads/main*"]);
    again.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_fetch_that_brings_an_older_commit_leaves_the_head_for_the_next_hand_over() {
    let place = Place::new("older-head");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    // GitHub hadn't moved `refs/pull/1/head` yet, so the fetch brought what it was before.
    let older = older(1);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older]);
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    let next = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(next.refspecs(), [refspec(1), refspec(2)]);
    next.succeed();
    requests.fetch_heads(cpython, vec![head(1), head(2)]);
    mirrors.stop(PLENTY);
    assert!(starts.recv().is_err(), "both heads landed");
}

#[test]
fn a_head_that_is_behind_is_fetched_once_more_after_a_moment_then_waits_for_a_hand_over() {
    let place = Place::new("behind-retry");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked_with(Duration::from_millis(50));
    let requests = mirrors.requests();
    let older = older(1);

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older.clone()]);
    let again = starts.recv_timeout(PLENTY).unwrap();
    assert_eq!(again.refspecs(), [refspec(1)]);
    again.succeed_with(vec![older]);
    requests.fetch_heads(cpython, vec![head(2)]);
    let next = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(
        next.refspecs(),
        [refspec(2)],
        "head 1 waits for a hand-over"
    );
    next.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_head_handed_over_during_its_fetch_that_comes_back_behind_isnt_asked_for_at_once() {
    let place = Place::new("behind-during");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython, vec![head(1)]);
    fetch.succeed_with(vec![older(1)]);
    mirrors.stop(PLENTY);

    assert!(starts.recv().is_err());
}

#[test]
fn a_head_github_doesnt_have_isnt_fetched_again() {
    let place = Place::new("missing-head");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![head(1)]);
    requests.fetch_heads(cpython, vec![head(1), head(2)]);
    mirrors.stop(PLENTY);

    assert!(starts.recv().is_err());
}

#[test]
fn heads_whose_refs_cant_be_read_back_are_fetched_at_the_next_hand_over() {
    let place = Place::new("unread-refs");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().succeed_unread();
    requests.fetch_heads(cpython, vec![head(1)]);
    let again = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(again.refspecs(), [refspec(1)]);
    again.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_fetch_brings_the_target_branches_of_its_heads_up_to_date_with_them() {
    let place = Place::new("target-branches");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let backport = PrHead {
        target_branch: "3.14".to_owned(),
        ..head(2)
    };

    mirrors
        .requests()
        .fetch_heads(cpython, vec![head(1), backport, head(3)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(
        fetch.branches(),
        [
            "+refs/heads/3.14*:refs/heads/3.14*",
            "+refs/heads/main*:refs/heads/main*"
        ]
    );
    fetch.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn an_opened_prs_head_goes_ahead_of_the_other_repositories_heads() {
    let place = Place::new("opened-first");
    let (cpython, hello) = (named("python/cpython"), named("octo-org/hello"));
    place.mirrored(&cpython);
    place.mirrored(&hello);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    let cpython_mirror = place.folders.mirror(&cpython);

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let first = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(hello, vec![head(5)]);
    requests.fetch_first(cpython, head(2));
    first.succeed();
    let opened = starts.recv_timeout(PLENTY).unwrap();

    assert!(
        opened
            .args
            .contains(&cpython_mirror.to_string_lossy().into_owned())
    );
    assert_eq!(opened.refspecs(), [refspec(2)]);
    assert_eq!(opened.branches(), ["+refs/heads/main*:refs/heads/main*"]);
    opened.succeed();
    let hellos = starts.recv_timeout(PLENTY).unwrap();
    assert_eq!(hellos.refspecs(), [refspec(5)]);
    hellos.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn repositories_take_turns_with_their_heads() {
    let place = Place::new("heads-turns");
    let (cpython, hello) = (named("python/cpython"), named("octo-org/hello"));
    place.mirrored(&cpython);
    place.mirrored(&hello);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    let hello_mirror = place.folders.mirror(&hello);

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let first = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(hello, vec![head(5)]);
    requests.fetch_heads(cpython, vec![head(2)]);
    first.succeed();
    let next = starts.recv_timeout(PLENTY).unwrap();

    assert!(
        next.args
            .contains(&hello_mirror.to_string_lossy().into_owned())
    );
    assert_eq!(next.refspecs(), [refspec(5)]);
    next.succeed();
    let last = starts.recv_timeout(PLENTY).unwrap();
    assert_eq!(last.refspecs(), [refspec(2)]);
    last.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn an_opened_prs_head_is_fetched_whatever_the_worker_noted_of_it() {
    let place = Place::new("opened-again");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().succeed_with(vec![]);
    requests.fetch_first(cpython, head(1));
    let opened = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(opened.refspecs(), [refspec(1)]);
    opened.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn an_opened_pr_wakes_a_stalled_worker() {
    let place = Place::new("opened-wakes");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().fail();
    requests.fetch_first(cpython, head(2));
    let opened = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(opened.refspecs(), [refspec(1), refspec(2)]);
    opened.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn the_app_hears_which_heads_arrived_and_not_those_behind_or_missing() {
    let place = Place::new("arrivals");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals) = place.start_faked_hearing(PLENTY);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1), head(2), head(3)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fetch.succeed_with(vec![head(1), older(2)]);
    requests.fetch_heads(cpython, vec![head(4)]);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older(4)]);
    mirrors.stop(PLENTY);

    let heard: Vec<_> = arrivals.iter().collect();
    assert_eq!(heard, [("python/cpython".to_owned(), vec![1])]);
}

#[test]
fn the_app_hears_which_heads_failed_and_why_once_they_wait_for_news() {
    let place = Place::new("failures");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, failures) = place.start_faked_hearing_failures();
    let requests = mirrors.requests();
    let unreachable = "fatal: unable to access 'https://github.com/python/cpython.git/': \
                       Could not resolve host: github.com";

    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    let first = starts.recv_timeout(PLENTY).unwrap();
    // Heads that come while the fetch runs are news, so the worker fetches again at once.
    requests.fetch_heads(cpython, vec![head(1), head(2)]);
    fail_saying(first, &format!("{unreachable}\n"));
    let again = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(again, &format!("{unreachable}\n"));
    mirrors.stop(PLENTY);

    let heard: Vec<_> = failures.iter().collect();
    let why = unreachable.to_owned();
    assert_eq!(heard, [("python/cpython".to_owned(), vec![1, 2], why)]);
}

#[test]
fn heads_wait_for_their_mirror_to_be_cloned() {
    let place = Place::new("heads-wait");
    let cpython = named("python/cpython");
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.clone_mirror(cpython.clone(), download());
    let clone = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    // What a real clone leaves, for the worker to move into place.
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    let fetch = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(fetch.refspecs(), [refspec(1)]);
    fetch.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn heads_without_a_mirror_wait_and_start_nothing() {
    let place = Place::new("no-mirror");
    let (mirrors, starts) = place.start_faked();

    mirrors
        .requests()
        .fetch_heads(named("python/cpython"), vec![head(1)]);
    mirrors.stop(PLENTY);

    assert!(starts.recv().is_err());
}

#[test]
fn quitting_stops_a_fetch_under_way() {
    let place = Place::new("stop-fetch");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();

    mirrors.requests().fetch_heads(cpython, vec![head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    mirrors.stop(PLENTY);

    assert_eq!(fetch.stopped.recv_timeout(PLENTY), Ok(()));
}

#[test]
fn a_fetch_that_cant_start_leaves_only_a_trace() {
    let place = Place::new("cant-fetch");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    drop(starts);
    let requests = mirrors.requests();

    // The second hand-over is the news a stalled worker waits for, and it tries once more.
    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    requests.fetch_heads(cpython, vec![head(1)]);
    mirrors.stop(PLENTY);
}

#[test]
fn offline_holds_the_worker() {
    let place = Place::new("offline");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.follow(&Status::Offline);
    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    requests.clone_mirror(named("octo-org/hello"), download());
    mirrors.stop(PLENTY);

    assert!(starts.recv().is_err());
}

#[test]
fn a_rate_limit_doesnt_hold_the_worker() {
    let place = Place::new("rate-limited");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.follow(&Status::OfflineAs {
        last_login: Login::new("octocat"),
    });
    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    requests.follow(&Status::Problem {
        problem: Error::new(
            ErrorKind::RateLimited { resets_at: None },
            "GitHub's rate limit",
        ),
    });
    let fetch = starts.recv_timeout(PLENTY).unwrap();

    assert_eq!(fetch.refspecs(), [refspec(1)]);
    fetch.succeed();
    mirrors.stop(PLENTY);
}

#[test]
fn a_fetch_brings_a_prs_head_into_the_mirror_and_a_head_github_lacks_fails_nothing_and_loses_its_ref()
 {
    let place = Place::new("real-heads");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let (mirrors, announced) = place.start();
    mirrors.requests().clone_mirror(hello.clone(), download());
    let _clone = one_clone(&announced);
    let commit = place.pull_request(&hello, 1);
    let mirror = place.folders.mirror(&hello);
    // What an earlier fetch left of a PR whose head GitHub has since dropped.
    let _ = git(&mirror, &["update-ref", "refs/tsugiki/pull/2/head", "HEAD"]);
    let dropped = mirror.join("refs/tsugiki/pull/2/head");

    let heads = vec![
        PrHead {
            commit: CommitSha::parse(&commit).unwrap(),
            ..head(1)
        },
        head(2),
    ];
    mirrors.requests().fetch_heads(hello.clone(), heads);
    let fetched = mirror.join("refs/tsugiki/pull/1/head");
    let deadline = Instant::now() + PLENTY;
    while (!fetched.exists() || dropped.exists()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    mirrors.stop(PLENTY);

    assert_eq!(
        git(&mirror, &["rev-parse", "refs/tsugiki/pull/1/head"]).trim(),
        commit
    );
    assert_eq!(
        git(
            &mirror,
            &["for-each-ref", "--format=%(refname)", "refs/tsugiki"]
        ),
        "refs/tsugiki/pull/1/head
"
    );
    let read = LiveGit.read(&[
        OsStr::new("-C"),
        mirror.as_os_str(),
        OsStr::new("for-each-ref"),
        OsStr::new("--format=%(objectname) %(refname)"),
        OsStr::new("refs/tsugiki/pull/"),
    ]);
    assert_eq!(
        pull_refs(&read.unwrap()),
        BTreeMap::from([(1, CommitSha::parse(&commit).unwrap())])
    );
}

#[test]
fn only_a_prs_head_ref_reads_as_where_its_head_landed() {
    let listing = format!(
        "{one} refs/tsugiki/pull/1/head
{one} refs/tsugiki/pull/2/head-x
not a commit          refs/tsugiki/pull/3/head
{one} refs/tsugiki/pull/four/head
",
        one = head(1).commit.as_str()
    );

    assert_eq!(pull_refs(&listing), BTreeMap::from([(1, head(1).commit)]));
}

mod borrowing;
mod upkeep;

mod problems {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const GIB: u64 = 1 << 30;
    /// Long enough for a git that would start to have started.
    const A_MOMENT: Duration = Duration::from_millis(300);

    /// Announces each problem the worker finds, and nothing else.
    fn hearing_problems() -> (Announce, Receiver<Option<MirrorProblem>>) {
        let (telling, told) = channel();
        let announce = Announce {
            problem: Box::new(move |problem| drop(telling.send(problem.cloned()))),
            ..Announce::cloning_only(Box::new(|_| {}))
        };
        (announce, told)
    }

    /// Whether `start` runs `git clone`. A fetch runs `git fetch`.
    fn clones(start: &FakeStart) -> bool {
        start.args.first().is_some_and(|arg| arg == "clone")
    }

    fn two_kilobytes() -> Box<dyn Transport> {
        Box::new(crate::github::testing::answered(
            200,
            r#"{"data":{"repository":{"diskUsage":2}}}"#,
        ))
    }

    fn empty() -> Box<dyn Transport> {
        Box::new(crate::github::testing::answered(
            200,
            r#"{"data":{"repository":{"diskUsage":null}}}"#,
        ))
    }

    #[test]
    fn github_gives_a_repositorys_size_in_kilobytes_or_nothing_when_its_empty() {
        let signed_in = crate::github::testing::signed_in_gh;
        let mut sized = sizes_on_github(two_kilobytes, signed_in);
        let mut empty = sizes_on_github(empty, signed_in);

        assert_eq!(sized(&named("octo-org/hello")).unwrap(), 2048);
        assert_eq!(empty(&named("octo-org/hello")).unwrap(), 0);
    }

    #[test]
    fn githubs_remotes_measure_the_disk_their_folder_is_on() {
        let place = Place::new("live-room");

        let free = (Remotes::github().free_space)(&place.folders.data).unwrap();
        // A connection to the API, which asks nothing until a size is wanted.
        drop(https());

        assert!(free > 0);
    }

    #[test]
    fn git_version_reads_the_major_and_minor_version() {
        assert_eq!(git_version("git version 2.43.0.windows.1\n"), Some((2, 43)));
        assert_eq!(
            git_version("git version 2.39.3 (Apple Git-146)"),
            Some((2, 39))
        );
        assert_eq!(git_version("git version 3"), None);
        assert_eq!(git_version("hub version 2.14.2"), None);
    }

    #[test]
    fn without_git_no_git_starts_until_a_check_finds_one() {
        let place = Place::new("no-git");
        let cpython = named("python/cpython");
        place.mirrored(&cpython);
        let (versions, answers) = channel();
        versions.send(None).unwrap();
        let (announce, told) = hearing_problems();
        let (mirrors, starts) = place.start_faked_from(place.clone_source(), announce, answers);

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        mirrors.requests().fetch_heads(cpython, vec![head(1)]);
        let missing = told.recv_timeout(PLENTY).unwrap();
        let held = starts.recv_timeout(A_MOMENT).is_err();
        mirrors.requests().check_again();
        let found = told.recv_timeout(PLENTY).unwrap();
        let fetch = starts.recv_timeout(PLENTY).unwrap();
        mirrors.stop(PLENTY);

        assert_eq!(missing, Some(MirrorProblem::NoGit));
        assert!(held);
        assert_eq!(found, None);
        assert!(!clones(&fetch), "{:?}", fetch.args);
    }

    #[test]
    fn a_git_older_than_2_30_is_named_by_what_it_says() {
        let place = Place::new("old-git");
        let (versions, answers) = channel();
        versions
            .send(Some("git version 2.29.2\n".to_owned()))
            .unwrap();
        let (announce, told) = hearing_problems();
        let (mirrors, starts) = place.start_faked_from(place.clone_source(), announce, answers);

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        let old = told.recv_timeout(PLENTY).unwrap();
        let held = starts.recv_timeout(A_MOMENT).is_err();
        mirrors.stop(PLENTY);

        let version = "git version 2.29.2".to_owned();
        assert_eq!(old, Some(MirrorProblem::OldGit { version }));
        assert!(held);
    }

    #[test]
    fn short_of_disk_a_clone_waits_for_a_check_and_a_fetch_goes_on() {
        let place = Place::new("disk-short");
        let [cpython, hello] = [named("python/cpython"), named("octo-org/hello")];
        place.mirrored(&cpython);
        let free = Arc::new(AtomicU64::new(GIB));
        let measured = Arc::clone(&free);
        let remotes = place.clone_source().on_disk(
            Box::new(|_| Ok(5 * GIB)),
            Box::new(move |_| Ok(measured.load(Ordering::SeqCst))),
        );
        let (announce, told) = hearing_problems();
        let (_versions, answers) = channel();
        let (mirrors, starts) = place.start_faked_from(remotes, announce, answers);

        mirrors.requests().clone_mirror(hello.clone(), download());
        let short = told.recv_timeout(PLENTY).unwrap();
        mirrors.requests().fetch_heads(cpython, vec![head(1)]);
        let fetch = starts.recv_timeout(PLENTY).unwrap();
        let fetched = !clones(&fetch);
        fetch.fail();
        free.store(10 * GIB, Ordering::SeqCst);
        mirrors.requests().check_again();
        let cleared = told.recv_timeout(PLENTY).unwrap();
        let clone = starts.recv_timeout(PLENTY).unwrap();
        mirrors.stop(PLENTY);

        let folder = place.folders.data.display().to_string();
        let needed = MirrorProblem::DiskShort {
            repository: hello,
            folder,
            needed_mb: 6 * 1024,
            free_mb: 1024,
        };
        assert_eq!(short, Some(needed));
        assert!(fetched);
        assert_eq!(cleared, None);
        assert!(clones(&clone), "{:?}", clone.args);
    }

    #[test]
    fn a_check_that_finds_the_disk_still_short_tells_the_window_nothing_new() {
        let place = Place::new("disk-still-short");
        let [cpython, hello] = [named("python/cpython"), named("octo-org/hello")];
        place.mirrored(&cpython);
        let remotes = place
            .clone_source()
            .on_disk(Box::new(|_| Ok(5 * GIB)), Box::new(|_| Ok(GIB)));
        let (announce, told) = hearing_problems();
        let (_versions, answers) = channel();
        let (mirrors, starts) = place.start_faked_from(remotes, announce, answers);

        mirrors.requests().clone_mirror(hello, download());
        let short = told.recv_timeout(PLENTY).unwrap();
        mirrors.requests().check_again();
        // The worker looks again before it reads the next request, so the fetch starts after the check.
        mirrors.requests().fetch_heads(cpython, vec![head(1)]);
        let fetch = starts.recv_timeout(PLENTY).unwrap();
        let heard = told.try_recv();
        let fetched = !clones(&fetch);
        fetch.fail();
        mirrors.stop(PLENTY);

        assert!(short.is_some());
        assert!(fetched);
        assert!(heard.is_err(), "{heard:?}");
    }

    #[test]
    fn a_clone_cut_off_by_quitting_is_deleted_before_the_disk_is_measured() {
        let place = Place::new("partial-measured");
        let hello = named("octo-org/hello");
        let partial = place.folders.repository(&hello).join(PARTIAL);
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join("half-a-pack"), "what a quit left").unwrap();
        let left = Arc::new(AtomicBool::new(true));
        let seen = Arc::clone(&left);
        let looked = partial.clone();
        let remotes = place.clone_source().on_disk(
            Box::new(|_| Ok(0)),
            Box::new(move |_| {
                seen.store(looked.exists(), Ordering::SeqCst);
                Ok(u64::MAX)
            }),
        );
        let (_versions, answers) = channel();
        let (mirrors, starts) =
            place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);

        mirrors.requests().clone_mirror(hello, download());
        let clone = starts.recv_timeout(PLENTY).unwrap();
        clone.fail();
        mirrors.stop(PLENTY);

        assert!(!left.load(Ordering::SeqCst));
    }

    #[test]
    fn a_clone_waiting_after_a_failure_starts_when_its_repository_opens_and_waits_in_one_place() {
        let place = Place::new("clone-one-place");
        let [cpython, hello] = [named("python/cpython"), named("octo-org/hello")];
        place.mirrored(&cpython);
        let remotes = place.clone_source().retrying(PLENTY * 10);
        let (_versions, answers) = channel();
        let (mirrors, starts) =
            place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);
        let requests = mirrors.requests();

        requests.clone_mirror(hello.clone(), download());
        let first = starts.recv_timeout(PLENTY).unwrap();
        let first_cloned = clones(&first);
        first.fail();
        requests.clone_mirror(hello.clone(), download());
        requests.clone_mirror_next(hello, download());
        let opened = starts.recv_timeout(PLENTY).unwrap();
        let opened_cloned = clones(&opened);
        opened.fail();
        requests.check_again();
        let checked = starts.recv_timeout(PLENTY).unwrap();
        let checked_cloned = clones(&checked);
        checked.fail();
        // A second place in line would start its clone before the worker reads this.
        requests.fetch_heads(cpython, vec![head(1)]);
        let next = starts.recv_timeout(PLENTY).unwrap();
        let next_cloned = clones(&next);
        let next_args = format!("{:?}", next.args);
        next.fail();
        mirrors.stop(PLENTY);

        assert!(first_cloned && opened_cloned && checked_cloned);
        assert!(!next_cloned, "{next_args}");
    }

    #[test]
    fn a_clone_that_keeps_failing_waits_twice_as_long_each_time_up_to_an_hour() {
        let minute = Duration::from_secs(60);

        assert_eq!(clone_wait(minute, 1), minute);
        assert_eq!(clone_wait(minute, 2), minute * 2);
        assert_eq!(clone_wait(minute, 3), minute * 4);
        assert_eq!(clone_wait(minute, 7), minute * 64);
        assert_eq!(clone_wait(minute, u32::MAX), minute * 64);
    }

    #[test]
    fn a_clone_that_fails_is_tried_again_after_a_while() {
        let place = Place::new("clone-retry");
        let remotes = place.clone_source().retrying(Duration::from_millis(10));
        let (_versions, answers) = channel();
        let (mirrors, starts) =
            place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        let first = starts.recv_timeout(PLENTY).unwrap();
        let cloned = clones(&first);
        first.fail();
        let again = starts.recv_timeout(PLENTY).unwrap();
        mirrors.stop(PLENTY);

        assert!(cloned && clones(&again));
    }

    #[test]
    fn a_size_github_cant_give_tries_the_clone_again_later() {
        let place = Place::new("size-retry");
        let mut asked = 0;
        let remotes = place
            .clone_source()
            .retrying(Duration::from_millis(10))
            .on_disk(
                Box::new(move |_| {
                    asked += 1;
                    if asked == 1 {
                        Err(Error::new(ErrorKind::Offline, "offline"))
                    } else {
                        Ok(0)
                    }
                }),
                Box::new(|_| Ok(u64::MAX)),
            );
        let (_versions, answers) = channel();
        let (mirrors, starts) =
            place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        let clone = starts.recv_timeout(PLENTY).unwrap();
        mirrors.stop(PLENTY);

        assert!(clones(&clone), "{:?}", clone.args);
    }

    #[test]
    fn a_disk_that_cant_be_measured_tries_the_clone_again_later() {
        let place = Place::new("free-retry");
        let asked = AtomicU64::new(0);
        let remotes = place
            .clone_source()
            .retrying(Duration::from_millis(10))
            .on_disk(
                Box::new(|_| Ok(0)),
                Box::new(move |_| {
                    if asked.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(io::Error::other("the disk went away"))
                    } else {
                        Ok(u64::MAX)
                    }
                }),
            );
        let (_versions, answers) = channel();
        let (mirrors, starts) =
            place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        let clone = starts.recv_timeout(PLENTY).unwrap();
        mirrors.stop(PLENTY);

        assert!(clones(&clone), "{:?}", clone.args);
    }

    #[test]
    fn an_app_that_hears_only_of_clones_still_starts_none_without_git() {
        let place = Place::new("no-git-quiet");
        let (versions, answers) = channel();
        versions.send(None).unwrap();
        let (mirrors, starts) = place.start_faked_from(
            place.clone_source(),
            Announce::cloning_only(Box::new(|_| {})),
            answers,
        );

        mirrors
            .requests()
            .clone_mirror(named("octo-org/hello"), download());
        let held = starts.recv_timeout(A_MOMENT).is_err();
        mirrors.stop(PLENTY);

        assert!(held);
    }
}
