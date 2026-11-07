use std::fs;
use std::io::{self, Read};
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

fn named(name: &str) -> RepositoryName {
    RepositoryName::parse(name).unwrap()
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
            Box::new(move |cloning| drop(announcing.send(cloning.cloned()))),
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

    mirrors.requests().clone_mirror(hello.clone());
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
fn a_clone_cut_off_by_quitting_starts_again_from_an_empty_folder() {
    let place = Place::new("restarts");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let partial = place
        .folders
        .mirror(&hello)
        .with_file_name("mirror.git.partial");
    fs::create_dir_all(&partial).unwrap();
    fs::write(partial.join("half-a-pack"), "what a quit left").unwrap();
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(hello.clone());
    let _ = one_clone(&announced);
    mirrors.stop(PLENTY);

    let mirror = place.folders.mirror(&hello);
    assert_eq!(git(&mirror, &["log", "--format=%s"]), "One\n");
    assert!(!mirror.join("half-a-pack").exists());
    assert!(!partial.exists());
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
        Box::new(move |cloning| drop(announcing.send(cloning.cloned()))),
    );
    let hello = named("octo-org/hello");

    mirrors.requests().clone_mirror(hello.clone());
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

    requests.clone_mirror(first.clone());
    requests.clone_mirror(second.clone());
    requests.clone_mirror(second.clone());
    requests.clone_mirror_next(first.clone());
    requests.clone_mirror(open.clone());
    requests.clone_mirror_next(open.clone());
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

    mirrors.requests().clone_mirror(cloned);
    mirrors.requests().clone_mirror_next(next.clone());
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

    mirrors.requests().clone_mirror(missing.clone());
    mirrors.requests().clone_mirror(next.clone());
    let failed = one_clone(&announced);
    let _ = one_clone(&announced);
    mirrors.stop(PLENTY);

    assert!(failed.iter().all(|cloning| cloning.repository == missing));
    let partial = place
        .folders
        .mirror(&missing)
        .with_file_name("mirror.git.partial");
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

    mirrors.requests().clone_mirror(named("octo-org/hello"));
    mirrors.stop(PLENTY);

    assert!(announced.try_recv().is_err());
}

#[test]
fn a_leftover_clone_that_cant_be_deleted_leaves_only_a_trace() {
    let place = Place::new("cant-delete");
    let hello = named("octo-org/hello");
    let partial = place.folders.repository(&hello).join("mirror.git.partial");
    fs::create_dir_all(partial.parent().unwrap()).unwrap();
    fs::write(&partial, "a file, which deleting a folder can't remove").unwrap();
    let (mirrors, announced) = place.start();

    mirrors.requests().clone_mirror(hello.clone());
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
    requests.clone_mirror(named("octo-org/hello"));

    assert!(announced.try_recv().is_err());
}

#[test]
fn without_its_data_folder_tsugiki_clones_nothing() {
    let (announcing, announced) = channel();
    let mirrors = Mirrors::start(
        Err(Error::new(ErrorKind::Internal, "There is no data folder.")),
        Remotes::github(),
        Box::new(move |cloning| drop(announcing.send(cloning.cloned()))),
    );

    mirrors.requests().clone_mirror(named("python/cpython"));
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
            .map(|PrHead { number, commit }| {
                format!(
                    "{} refs/tsugiki/pull/{number}/head
",
                    commit.as_str()
                )
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

    fn refspecs(&self) -> Vec<&str> {
        self.input.lines().collect()
    }
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

    fn read(&self, _args: &[&OsStr]) -> io::Result<String> {
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
        let (starting, starts) = channel();
        let (landed, refs) = channel();
        let mirrors = Mirrors::with_git(
            Box::new(FakeGit {
                starts: starting,
                landed,
                refs,
            }),
            Ok(self.folders.clone()),
            self.clone_source().patient(patience),
            Box::new(|_| {}),
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
    let older = PrHead {
        number: 1,
        commit: head(9).commit,
    };
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
    let older = PrHead {
        number: 1,
        commit: head(9).commit,
    };

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
    fetch.succeed_with(vec![PrHead {
        number: 1,
        commit: head(9).commit,
    }]);
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
fn heads_wait_for_their_mirror_to_be_cloned() {
    let place = Place::new("heads-wait");
    let cpython = named("python/cpython");
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.clone_mirror(cpython.clone());
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
    requests.clone_mirror(named("octo-org/hello"));
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
    mirrors.requests().clone_mirror(hello.clone());
    let _clone = one_clone(&announced);
    let commit = place.pull_request(&hello, 1);
    let mirror = place.folders.mirror(&hello);
    // What an earlier fetch left of a PR whose head GitHub has since dropped.
    let _ = git(&mirror, &["update-ref", "refs/tsugiki/pull/2/head", "HEAD"]);
    let dropped = mirror.join("refs/tsugiki/pull/2/head");

    let heads = vec![
        PrHead {
            number: 1,
            commit: CommitSha::parse(&commit).unwrap(),
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
