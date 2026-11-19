//! The per-file patch-ids the Stale review check compares (#383). One test reads them in a real mirror, and
//! the rest run on the fake git.

use super::*;
use crate::facts::patch_ids::{CantCheck, CommitPatches, PatchesOf, UncheckedCommit};
use crate::patch_ids::PatchReads;

/// Each Repository's patch-ids the worker handed over, by name.
type Patched = Receiver<(String, Vec<CommitPatches>)>;

/// The commits the worker said it can't read, each time that changed.
type Unchecked = Receiver<Vec<UncheckedCommit>>;

/// Hands what `mirrors` hands over of patch-ids to the first receiver, and the commits it can't read to the
/// second.
fn hear_patches(mirrors: &Mirrors) -> (Patched, Unchecked) {
    let (patching, patched) = channel();
    let (unchecking, unchecked) = channel();
    mirrors
        .requests()
        .hand_patches_to(Box::new(move |repository, reads| match reads {
            PatchReads::Read(read) => drop(patching.send((repository.to_string(), read))),
            PatchReads::Unchecked(commits) => drop(unchecking.send(commits)),
        }));
    (patched, unchecked)
}

/// The worker with the fake git, and what a test hears of it.
struct Faked {
    mirrors: Mirrors,
    starts: Receiver<FakeStart>,
    /// Each read of a commit's patch-ids, which waits for the test to answer it.
    reads: Receiver<FakePatchRead>,
    patched: Patched,
    unchecked: Unchecked,
}

impl Faked {
    /// Waits for the worker's next read of patch-ids.
    fn next_read(&self) -> FakePatchRead {
        self.reads.recv_timeout(PLENTY).unwrap()
    }
}

impl Place {
    /// A real mirror of a new remote for `name`, with PR 1 on the remote, the worker that cloned it, what
    /// the worker hands over of patch-ids, and the commits it can't read. Gives PR 1's commit, which the
    /// mirror lacks.
    fn cloned_hearing_patches(
        &self,
        name: &RepositoryName,
    ) -> (Mirrors, Patched, Unchecked, CommitSha) {
        self.remote(name);
        let pull_request = CommitSha::parse(&self.pull_request(name, 1)).unwrap();
        let (announcing, announced) = channel();
        let announce = cloning_only(announcing);
        let mirrors = Mirrors::start(Ok(self.folders.clone()), self.clone_source(), announce);
        let (patched, unchecked) = hear_patches(&mirrors);
        mirrors.requests().clone_mirror(name.clone(), download());
        clone_ended(&announced);
        (mirrors, patched, unchecked, pull_request)
    }

    /// The worker with the fake git, which hands what it reads of patch-ids to the test.
    fn faked_hearing_patches(&self) -> Faked {
        let (mirrors, starts, reads) = self.start_faked_reading();
        let (patched, unchecked) = hear_patches(&mirrors);
        Faked {
            mirrors,
            starts,
            reads,
            patched,
            unchecked,
        }
    }
}

fn on_main(commit: &CommitSha) -> PatchesOf {
    PatchesOf {
        commit: commit.clone(),
        target_branch: "main".to_owned(),
    }
}

/// PR 1's head at `commit`, into main.
fn head_at(commit: &CommitSha) -> PrHead {
    PrHead {
        number: 1,
        commit: commit.clone(),
        target_branch: "main".to_owned(),
    }
}

/// The paths of each commit's files, by commit.
fn paths(read: &[CommitPatches]) -> BTreeMap<CommitSha, Vec<String>> {
    let paths = read.iter().map(|patches| {
        let files = patches.files.iter().map(|file| file.path.clone());
        (patches.of.commit.clone(), files.collect())
    });
    paths.collect()
}

#[test]
fn the_worker_reads_the_patch_ids_of_commits_its_mirror_has_and_hands_them_over() {
    let place = Place::new("patches");
    let cpython = named("python/cpython");
    let (mirrors, patched, _, pull_request) = place.cloned_hearing_patches(&cpython);

    mirrors
        .requests()
        .fetch_heads(cpython.clone(), vec![head_at(&pull_request)]);
    let wanted = vec![on_main(&pull_request)];
    mirrors.requests().read_patch_ids(cpython.clone(), wanted);
    let (repository, read) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(repository, "python/cpython");
    assert_eq!(
        paths(&read),
        BTreeMap::from([(pull_request, vec!["NEWS".to_owned()])])
    );
}

#[test]
fn the_commits_of_one_hand_over_are_handed_over_together() {
    let place = Place::new("patches-together");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let [first, second] = [1, 2].map(|number| head(number).commit);

    let wanted = vec![on_main(&first), on_main(&second)];
    faked.mirrors.requests().read_patch_ids(cpython, wanted);
    faked.next_read().give(&["NEWS"]);
    let second_read = faked.next_read();
    let early = faked.patched.try_recv();
    second_read.give(&["NEWS"]);
    let (repository, read) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert!(early.is_err(), "{early:?}");
    assert_eq!(repository, "python/cpython");
    assert_eq!(
        paths(&read).into_keys().collect::<BTreeSet<_>>(),
        BTreeSet::from([first, second])
    );
}

#[test]
fn a_clone_asked_for_during_the_reads_starts_once_they_end() {
    let place = Place::new("patches-then-clone");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let [first, second] = [1, 2].map(|number| head(number).commit);

    let wanted = vec![on_main(&first), on_main(&second)];
    faked.mirrors.requests().read_patch_ids(cpython, wanted);
    let first_read = faked.next_read();
    // It waits for the reads, and comes while the first runs, so no message follows the last.
    faked
        .mirrors
        .requests()
        .clone_mirror(named("octo-org/hello"), download());
    first_read.give(&["NEWS"]);
    faked.next_read().give(&["NEWS"]);
    let clone = faked.starts.recv_timeout(PLENTY).unwrap();
    let cloned = clone.cloned();
    clone.fail();
    faked.mirrors.stop_cleanly(PLENTY);

    assert_eq!(cloned.to_string(), "octo-org/hello");
}

#[test]
fn a_commit_asked_for_again_after_its_hand_over_is_read_again_as_its_save_may_have_failed() {
    let place = Place::new("patches-again");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let requests = faked.mirrors.requests();
    let commit = head(1).commit;
    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    faked.next_read().give(&["NEWS"]);
    let _first = faked.patched.recv_timeout(PLENTY).unwrap();

    // The store holds what it saved, and the session asks only for what it lacks.
    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    faked.next_read().give(&["NEWS"]);
    let (_, again) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert_eq!(paths(&again).into_keys().collect::<Vec<_>>(), [commit]);
}

#[test]
fn a_hand_over_during_the_reads_hands_over_what_they_read_and_doesnt_read_it_again() {
    let place = Place::new("patches-between");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let requests = faked.mirrors.requests();
    let [first, second] = [1, 2].map(|number| head(number).commit);
    let both = vec![on_main(&first), on_main(&second)];

    // The next hand-over comes while the worker reads one of the two, and asks for both again.
    requests.read_patch_ids(cpython.clone(), both.clone());
    let one = faked.next_read();
    requests.read_patch_ids(cpython.clone(), both);
    let one_commit = one.of.commit.clone();
    one.give(&["NEWS"]);
    let (_, handed) = faked.patched.recv_timeout(PLENTY).unwrap();
    let other = faked.next_read();
    let other_commit = other.of.commit.clone();
    other.give(&["NEWS"]);
    let (_, then) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.wait_idle();
    let read_again = faked.reads.try_recv().is_ok();
    let after = faked.patched.try_recv();
    faked.mirrors.stop_cleanly(PLENTY);

    assert_eq!(
        BTreeSet::from([&one_commit, &other_commit]),
        BTreeSet::from([&first, &second])
    );
    let handed_commits: Vec<CommitSha> = paths(&handed).into_keys().collect();
    assert_eq!(
        handed_commits,
        [one_commit],
        "the hand-over takes what was read"
    );
    assert_eq!(paths(&then).into_keys().collect::<Vec<_>>(), [other_commit]);
    assert!(!read_again, "each commit is read once");
    assert!(after.is_err(), "{after:?}");
}

#[test]
fn a_reviewed_commit_the_mirror_lacks_is_read_once_its_fetch_brings_it() {
    let place = Place::new("patches-reviewed");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let requests = faked.mirrors.requests();
    let commit = head(1).commit;

    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    faked.next_read().fail();
    faked.mirrors.wait_idle();
    let before = faked.patched.try_recv();
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    faked.starts.recv_timeout(PLENTY).unwrap().succeed_unread();
    faked.next_read().give(&["NEWS"]);
    let (_, read) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert!(before.is_err(), "{before:?}");
    assert_eq!(
        paths(&read),
        BTreeMap::from([(commit, vec!["NEWS".to_owned()])])
    );
}

#[test]
fn a_commit_the_mirror_lacks_is_read_once_a_fetch_brings_it() {
    let place = Place::new("patches-later");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let requests = faked.mirrors.requests();
    let commit = head(1).commit;

    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    faked.next_read().fail();
    faked.mirrors.wait_idle();
    let before = faked.patched.try_recv();
    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    faked.starts.recv_timeout(PLENTY).unwrap().succeed();
    faked.next_read().give(&["NEWS"]);
    let (_, read) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert!(before.is_err(), "{before:?}");
    assert_eq!(
        paths(&read),
        BTreeMap::from([(commit, vec!["NEWS".to_owned()])])
    );
    assert!(
        faked.unchecked.try_recv().is_err(),
        "a read that waits for a fetch has no reason"
    );
}

fn cant_check(commit: &CommitSha, reason: CantCheck) -> Vec<UncheckedCommit> {
    vec![UncheckedCommit {
        of: on_main(commit),
        reason,
    }]
}

#[test]
fn a_commit_cant_be_checked_until_its_mirror_is_cloned_and_is_read_after() {
    let place = Place::new("patches-cloning");
    let cpython = named("python/cpython");
    let faked = place.faked_hearing_patches();
    let requests = faked.mirrors.requests();
    let commit = head(1).commit;

    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    let cloning = faked.unchecked.recv_timeout(PLENTY).unwrap();
    requests.clone_mirror(cpython.clone(), download());
    let clone = faked.starts.recv_timeout(PLENTY).unwrap();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    faked.next_read().give(&["NEWS"]);
    let cloned = faked.unchecked.recv_timeout(PLENTY).unwrap();
    let (_, read) = faked.patched.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert_eq!(cloning, cant_check(&commit, CantCheck::Cloning));
    assert_eq!(cloned, []);
    assert_eq!(paths(&read).into_keys().collect::<Vec<_>>(), [commit]);
}

#[test]
fn a_commit_cant_be_checked_while_its_broken_mirror_is_cloned_again() {
    let place = Place::new("patches-broken");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let (_patched, unchecked) = hear_patches(&mirrors);
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());
    let commit = head(1).commit;
    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(
        fetch,
        "fatal: bad object 0123456789abcdef0123456789abcdef01234567\n",
    );
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let broken = unchecked.recv_timeout(PLENTY).unwrap();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    let cloned = unchecked.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(broken, cant_check(&commit, CantCheck::Broken));
    assert_eq!(cloned, []);
}

#[test]
fn a_commit_cant_be_checked_without_a_git_new_enough_and_the_reason_says_which() {
    let old = |version: &str, needed: &str| CantCheck::OldGit {
        version: version.to_owned(),
        needed: needed.to_owned(),
    };
    let gits = [
        (None, CantCheck::NoGit),
        // Only the newer git the check needs is worth naming.
        (
            Some("git version 2.29.2"),
            old("git version 2.29.2", "2.39"),
        ),
        (
            Some("git version 2.38.1.windows.1"),
            old("git version 2.38.1.windows.1", "2.39"),
        ),
    ];
    for (at, (said, reason)) in gits.into_iter().enumerate() {
        let place = Place::new(&format!("patches-git-{at}"));
        let cpython = named("python/cpython");
        place.mirrored(&cpython);
        let (versions, answers) = channel();
        versions.send(said.map(|said| format!("{said}\n"))).unwrap();
        let announce = Announce::cloning_only(Box::new(|_| {}));
        let (mirrors, _starts) = place.start_faked_from(place.clone_source(), announce, answers);
        let (patched, unchecked) = hear_patches(&mirrors);
        let commit = head(1).commit;

        mirrors
            .requests()
            .read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
        let told = unchecked.recv_timeout(PLENTY).unwrap();
        mirrors.wait_idle();
        let read = patched.try_recv();
        mirrors.stop_cleanly(PLENTY);

        assert_eq!(told, cant_check(&commit, reason), "{said:?}");
        assert!(read.is_err(), "{read:?}");
    }
}

#[test]
fn a_reviewed_commit_github_refused_to_send_cant_be_checked() {
    let place = Place::new("patches-refused");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let faked = place.faked_hearing_patches();
    let commit = head(1).commit;
    let refused = place
        .folders
        .repository(&cpython)
        .join("refused-commits.txt");
    fs::write(refused, format!("{}\n", commit.as_str())).unwrap();

    let requests = faked.mirrors.requests();
    requests.read_patch_ids(cpython.clone(), vec![on_main(&commit)]);
    faked.next_read().fail();
    let told = faked.unchecked.recv_timeout(PLENTY).unwrap();
    requests.read_patch_ids(cpython.clone(), vec![]);
    let forgot = faked.unchecked.recv_timeout(PLENTY).unwrap();
    faked.mirrors.stop_cleanly(PLENTY);

    assert_eq!(told, cant_check(&commit, CantCheck::Refused));
    assert_eq!(
        forgot,
        [],
        "a hand-over without the commit drops its reason"
    );
}
