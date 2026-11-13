//! The per-file patch-ids the Stale review check compares, read in a real mirror (#383).

use super::*;
use crate::patch_ids::{CantCheck, CommitPatches, PatchReads, PatchesOf, UncheckedCommit};

/// Long enough for a read that was going to happen to have happened.
const A_MOMENT: Duration = Duration::from_millis(500);

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
        let _clone = one_clone(&announced);
        (mirrors, patched, unchecked, pull_request)
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
    let (mirrors, patched, _, first) = place.cloned_hearing_patches(&cpython);
    let second = CommitSha::parse(&place.pull_request(&cpython, 2)).unwrap();
    let requests = mirrors.requests();
    let second_head = PrHead {
        number: 2,
        ..head_at(&second)
    };
    requests.fetch_heads(cpython.clone(), vec![head_at(&first), second_head]);

    requests.read_patch_ids(cpython.clone(), vec![on_main(&first), on_main(&second)]);
    let (_, read) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(
        paths(&read).into_keys().collect::<BTreeSet<_>>(),
        BTreeSet::from([first, second])
    );
}

/// PR `number`'s head at `commit`, into main.
fn pr_head(number: u32, commit: &CommitSha) -> PrHead {
    PrHead {
        number,
        ..head_at(commit)
    }
}

#[test]
fn a_commit_asked_for_again_after_its_hand_over_is_read_again_as_its_save_may_have_failed() {
    let place = Place::new("patches-again");
    let cpython = named("python/cpython");
    let (mirrors, patched, _, pull_request) = place.cloned_hearing_patches(&cpython);
    let requests = mirrors.requests();
    requests.fetch_heads(cpython.clone(), vec![head_at(&pull_request)]);
    requests.read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let _first = patched.recv_timeout(PLENTY).unwrap();

    // The store holds what it saved, and the session asks only for what it lacks.
    requests.read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let (_, again) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(
        paths(&again).into_keys().collect::<Vec<_>>(),
        [pull_request]
    );
}

#[test]
fn a_hand_over_during_the_reads_hands_over_what_they_read_and_doesnt_read_it_again() {
    let place = Place::new("patches-between");
    let cpython = named("python/cpython");
    let (mirrors, patched, _, first) = place.cloned_hearing_patches(&cpython);
    let [second, third] =
        [2, 3].map(|number| CommitSha::parse(&place.pull_request(&cpython, number)).unwrap());
    let requests = mirrors.requests();
    let heads = vec![head_at(&first), pr_head(2, &second), pr_head(3, &third)];
    requests.fetch_heads(cpython.clone(), heads);
    // Once this arrives, the heads are in the mirror and the worker has nothing to do.
    requests.read_patch_ids(cpython.clone(), vec![on_main(&first)]);
    let _fetched = patched.recv_timeout(PLENTY).unwrap();

    // The worker reads one of the two before it takes the next hand-over, which asks for both again.
    let both = vec![on_main(&second), on_main(&third)];
    requests.read_patch_ids(cpython.clone(), both.clone());
    requests.read_patch_ids(cpython.clone(), both);
    let (_, one) = patched.recv_timeout(PLENTY).unwrap();
    let (_, other) = patched.recv_timeout(PLENTY).unwrap();
    let after = patched.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    let read: Vec<CommitSha> = [one, other]
        .iter()
        .flat_map(|read| paths(read).into_keys())
        .collect();
    assert_eq!(
        read.iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([&second, &third])
    );
    assert_eq!(read.len(), 2, "each read once: {read:?}");
    assert!(after.is_err(), "{after:?}");
}

#[test]
fn a_reviewed_commit_the_mirror_lacks_is_read_once_its_fetch_brings_it() {
    let place = Place::new("patches-reviewed");
    let cpython = named("python/cpython");
    let (mirrors, patched, _, pull_request) = place.cloned_hearing_patches(&cpython);
    let requests = mirrors.requests();

    requests.read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let before = patched.recv_timeout(A_MOMENT);
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &pull_request)]);
    let (_, read) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert!(before.is_err(), "{before:?}");
    assert_eq!(
        paths(&read),
        BTreeMap::from([(pull_request, vec!["NEWS".to_owned()])])
    );
}

#[test]
fn a_commit_the_mirror_lacks_is_read_once_a_fetch_brings_it() {
    let place = Place::new("patches-later");
    let cpython = named("python/cpython");
    let (mirrors, patched, unchecked, pull_request) = place.cloned_hearing_patches(&cpython);

    mirrors
        .requests()
        .read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let before = patched.recv_timeout(A_MOMENT);
    mirrors
        .requests()
        .fetch_heads(cpython.clone(), vec![head_at(&pull_request)]);
    let (_, read) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert!(before.is_err(), "{before:?}");
    assert_eq!(
        paths(&read),
        BTreeMap::from([(pull_request, vec!["NEWS".to_owned()])])
    );
    assert!(
        unchecked.try_recv().is_err(),
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
    place.remote(&cpython);
    let pull_request = CommitSha::parse(&place.pull_request(&cpython, 1)).unwrap();
    let (announcing, announced) = channel();
    let announce = cloning_only(announcing);
    let mirrors = Mirrors::start(Ok(place.folders.clone()), place.clone_source(), announce);
    let (patched, unchecked) = hear_patches(&mirrors);
    let requests = mirrors.requests();

    requests.read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let cloning = unchecked.recv_timeout(PLENTY).unwrap();
    requests.clone_mirror(cpython.clone(), download());
    let _clone = one_clone(&announced);
    let cloned = unchecked.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython.clone(), vec![head_at(&pull_request)]);
    let (_, read) = patched.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(cloning, cant_check(&pull_request, CantCheck::Cloning));
    assert_eq!(cloned, []);
    assert_eq!(paths(&read).into_keys().collect::<Vec<_>>(), [pull_request]);
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
        let read = patched.recv_timeout(A_MOMENT);
        mirrors.stop_cleanly(PLENTY);

        assert_eq!(told, cant_check(&commit, reason), "{said:?}");
        assert!(read.is_err(), "{read:?}");
    }
}

#[test]
fn a_reviewed_commit_github_refused_to_send_cant_be_checked() {
    let place = Place::new("patches-refused");
    let cpython = named("python/cpython");
    let (mirrors, _patched, unchecked, pull_request) = place.cloned_hearing_patches(&cpython);
    let refused = place
        .folders
        .repository(&cpython)
        .join("refused-commits.txt");
    fs::write(refused, format!("{}\n", pull_request.as_str())).unwrap();

    mirrors
        .requests()
        .read_patch_ids(cpython.clone(), vec![on_main(&pull_request)]);
    let told = unchecked.recv_timeout(PLENTY).unwrap();
    mirrors.requests().read_patch_ids(cpython.clone(), vec![]);
    let forgot = unchecked.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(told, cant_check(&pull_request, CantCheck::Refused));
    assert_eq!(
        forgot,
        [],
        "a hand-over without the commit drops its reason"
    );
}
