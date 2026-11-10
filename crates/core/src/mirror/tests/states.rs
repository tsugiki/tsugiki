//! The state of each Repository's mirror, which the Repository picker shows (#320).

use super::*;

/// The states of the mirrors, as the worker announces them.
type States = Receiver<Vec<RepositoryMirror>>;

const GIB: u64 = 1 << 30;

impl Place {
    /// The worker with the fake git, cloning from `remotes`, each start of it, and each list of states it
    /// announces. Each `git --version` takes the next of `versions`, as in `start_faked_from`.
    fn start_hearing_states(
        &self,
        remotes: Remotes,
        versions: Receiver<Option<String>>,
    ) -> (Mirrors, Receiver<FakeStart>, States) {
        let (telling, told) = channel();
        let announce = Announce {
            states: Box::new(move |states| drop(telling.send(states.to_vec()))),
            ..Announce::cloning_only(Box::new(|_| {}))
        };
        let (mirrors, starts) = self.start_faked_from(remotes, announce, versions);
        (mirrors, starts, told)
    }

    /// The worker as `start_hearing_states` starts it, whose failed clones wait longer than a test, with a
    /// git 2.43.
    fn start_hearing(&self) -> (Mirrors, Receiver<FakeStart>, States) {
        let (_versions, answers) = channel();
        self.start_hearing_states(self.clone_source().retrying(PLENTY * 10), answers)
    }
}

fn mirror(repository: &RepositoryName, state: MirrorState) -> RepositoryMirror {
    RepositoryMirror {
        repository: repository.clone(),
        state,
    }
}

fn waiting_behind(repository: &RepositoryName) -> MirrorState {
    MirrorState::Waiting {
        behind: Some(repository.clone()),
    }
}

fn cloning(progress: CloneProgress) -> MirrorState {
    MirrorState::Cloning { progress }
}

/// Waits for the worker to announce `expected`, and fails with what it announced last if it doesn't.
fn wait_for(told: &States, expected: &[RepositoryMirror]) {
    let mut last = None;
    while let Ok(states) = told.recv_timeout(PLENTY) {
        if states == expected {
            return;
        }
        last = Some(states);
    }
    panic!("the worker announced {last:?}, not {expected:?}");
}

#[test]
fn each_mirror_says_if_it_is_there_cloning_or_waiting_behind_the_clone_under_way() {
    let place = Place::new("states");
    let [ready, first, second] = [
        named("octo-org/ready"),
        named("octo-org/first"),
        named("octo-org/second"),
    ];
    place.mirrored(&ready);
    let (mirrors, starts, told) = place.start_hearing();
    let requests = mirrors.requests();

    requests.clone_mirror(ready.clone(), download());
    requests.clone_mirror(first.clone(), download());
    requests.clone_mirror(second.clone(), download());
    let mut clone = starts.recv_timeout(PLENTY).unwrap();
    wait_for(
        &told,
        &[
            mirror(&ready, MirrorState::Ready),
            mirror(&first, cloning(CloneProgress::Starting)),
            mirror(&second, waiting_behind(&first)),
        ],
    );
    clone
        .output
        .write_all(b"Receiving objects:  40% (4/10)\r")
        .unwrap();
    wait_for(
        &told,
        &[
            mirror(&ready, MirrorState::Ready),
            mirror(&first, cloning(CloneProgress::Receiving { percent: 40 })),
            mirror(&second, waiting_behind(&first)),
        ],
    );
    fail_saying(clone, "fatal: repository not found\n");
    let next = starts.recv_timeout(PLENTY).unwrap();
    let failed = MirrorState::Failed {
        words: "fatal: repository not found".to_owned(),
    };
    wait_for(
        &told,
        &[
            mirror(&ready, MirrorState::Ready),
            mirror(&first, failed.clone()),
            mirror(&second, cloning(CloneProgress::Starting)),
        ],
    );
    // What git cloned, which the worker moves to the mirror's place.
    fs::create_dir_all(place.folders.repository(&second).join(PARTIAL)).unwrap();
    next.succeed();
    wait_for(
        &told,
        &[
            mirror(&ready, MirrorState::Ready),
            mirror(&first, failed),
            mirror(&second, MirrorState::Ready),
        ],
    );
    mirrors.stop_cleanly(PLENTY);
}

#[test]
fn a_mirror_that_turns_up_while_its_clone_waits_is_ready_and_isnt_cloned() {
    let place = Place::new("states-turns-up");
    let [first, second] = [named("octo-org/first"), named("octo-org/second")];
    let (mirrors, starts, told) = place.start_hearing();

    mirrors.requests().clone_mirror(first.clone(), download());
    mirrors.requests().clone_mirror(second.clone(), download());
    let clone = starts.recv_timeout(PLENTY).unwrap();
    wait_for(
        &told,
        &[
            mirror(&first, cloning(CloneProgress::Starting)),
            mirror(&second, waiting_behind(&first)),
        ],
    );
    // Another copy of Tsugiki on these folders cloned it meanwhile.
    place.mirrored(&second);
    clone.fail();
    let failed = MirrorState::Failed {
        words: String::new(),
    };
    wait_for(
        &told,
        &[mirror(&first, failed), mirror(&second, MirrorState::Ready)],
    );
    mirrors.stop_cleanly(PLENTY);

    assert!(starts.try_recv().is_err(), "a second clone started");
}

#[test]
fn a_clone_that_waits_for_an_answer_says_so_until_the_answer_comes() {
    let place = Place::new("states-asking");
    let hello = named("octo-org/hello");
    let (mirrors, starts, told) = place.start_hearing();

    let requests = mirrors.requests();
    assert!(!requests.asks(&hello));

    requests.clone_mirror(hello.clone(), None);
    wait_for(&told, &[mirror(&hello, MirrorState::Asking)]);
    assert!(requests.asks(&named("Octo-Org/Hello")));
    requests.clone_mirror(hello.clone(), download());
    let _clone = starts.recv_timeout(PLENTY).unwrap();
    wait_for(&told, &[mirror(&hello, cloning(CloneProgress::Starting))]);
    assert!(!requests.asks(&hello));
    mirrors.stop_cleanly(PLENTY);
}

#[test]
fn without_git_each_mirror_that_isnt_there_names_the_problem() {
    let place = Place::new("states-no-git");
    let [ready, hello] = [named("octo-org/ready"), named("octo-org/hello")];
    place.mirrored(&ready);
    let (versions, answers) = channel();
    versions.send(None).unwrap();
    let (mirrors, _starts, told) = place.start_hearing_states(place.clone_source(), answers);

    mirrors.requests().clone_mirror(ready.clone(), download());
    mirrors.requests().clone_mirror(hello.clone(), download());
    let no_git = MirrorState::Problem {
        problem: MirrorProblem::NoGit,
    };
    wait_for(
        &told,
        &[mirror(&ready, MirrorState::Ready), mirror(&hello, no_git)],
    );
    mirrors.stop_cleanly(PLENTY);
}

#[test]
fn short_of_disk_each_clone_waiting_names_the_problem() {
    let place = Place::new("states-disk-short");
    let [hello, peps] = [named("octo-org/hello"), named("python/peps")];
    let remotes = place
        .clone_source()
        .on_disk(Box::new(|_| Ok(5 * GIB)), Box::new(|_| Ok(GIB)));
    let (_versions, answers) = channel();
    let (mirrors, _starts, told) = place.start_hearing_states(remotes, answers);

    mirrors.requests().clone_mirror(hello.clone(), download());
    mirrors.requests().clone_mirror(peps.clone(), download());
    // Ahead of the clone the disk is short for, it still waits for room.
    mirrors
        .requests()
        .clone_mirror_next(peps.clone(), download());
    let short = MirrorState::Problem {
        problem: MirrorProblem::DiskShort {
            repository: hello.clone(),
            folder: place.folders.data.display().to_string(),
            needed_mb: 6 * 1024,
            free_mb: 1024,
        },
    };
    wait_for(
        &told,
        &[mirror(&hello, short.clone()), mirror(&peps, short)],
    );
    mirrors.stop_cleanly(PLENTY);
}

#[test]
fn offline_a_clone_waits_with_no_clone_ahead_of_it() {
    let place = Place::new("states-offline");
    let hello = named("octo-org/hello");
    let (mirrors, _starts, told) = place.start_hearing();

    mirrors.requests().follow(&Status::Offline);
    mirrors.requests().clone_mirror(hello.clone(), download());
    wait_for(
        &told,
        &[mirror(&hello, MirrorState::Waiting { behind: None })],
    );
    mirrors.stop_cleanly(PLENTY);
}

#[test]
fn the_window_hears_the_states_only_when_one_changes() {
    let place = Place::new("states-once");
    let ready = named("octo-org/ready");
    place.mirrored(&ready);
    let (mirrors, _starts, told) = place.start_hearing();

    mirrors.requests().clone_mirror(ready.clone(), download());
    wait_for(&told, &[mirror(&ready, MirrorState::Ready)]);
    mirrors.requests().clone_mirror_next(ready, download());
    mirrors.requests().check_again();
    mirrors.stop_cleanly(PLENTY);
    let again = told.try_recv();

    assert!(
        again.is_err(),
        "the window heard the same states again: {again:?}"
    );
}
