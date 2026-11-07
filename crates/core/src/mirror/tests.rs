use std::fs;
use std::io::{self, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use super::*;
use crate::error::ErrorKind;

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
