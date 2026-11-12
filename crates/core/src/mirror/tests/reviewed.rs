//! The commit each of the user's Reviews covered, kept under a reviewed ref (#382).

use super::*;

/// Long enough for a git that was going to start to have started.
const A_MOMENT: Duration = Duration::from_millis(300);

/// What GitHub says of a commit it won't send.
const NOT_OUR_REF: &str = "fatal: remote error: upload-pack: not our ref";

impl Place {
    /// A commit of PR `number` that the remote for `name` has on no ref, as after a force-push dropped it
    /// before the mirror fetched it.
    fn dropped_commit(&self, name: &RepositoryName, number: u32) -> CommitSha {
        let commit = self.pull_request(name, number);
        let bare = self.remotes.join(format!("{name}.git"));
        let _ = git(
            &bare,
            &["update-ref", "-d", &format!("refs/pull/{number}/head")],
        );
        CommitSha::parse(&commit).unwrap()
    }

    /// Where the reviewed ref of PR `number` in `repository`'s real mirror points, if it is there.
    fn reviewed_ref(&self, repository: &RepositoryName, number: u32) -> Option<CommitSha> {
        let mirror = self.folders.mirror(repository);
        let name = format!("refs/tsugiki/reviewed/{number}");
        let read = LiveGit.read(&[
            OsStr::new("-C"),
            mirror.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("--quiet"),
            OsStr::new(&name),
        ]);
        CommitSha::parse(read.ok()?.trim())
    }

    /// The commits the worker noted that GitHub refused to send, as the file beside the mirror lists them.
    fn refused(&self, repository: &RepositoryName) -> String {
        let file = self
            .folders
            .repository(repository)
            .join("refused-commits.txt");
        fs::read_to_string(file).unwrap_or_default()
    }

    /// An empty mirror that is really there, for the real git to look in while the fake one runs the rest.
    fn bare_mirror(&self, repository: &RepositoryName) {
        let mirror = self.folders.mirror(repository);
        fs::create_dir_all(&mirror).unwrap();
        let _ = git(&mirror, &["init", "-q", "--bare"]);
    }

    /// A mirror cloned by hand from a new remote for `repository`, and the one commit it has.
    fn mirror_with_a_commit(&self, repository: &RepositoryName) -> CommitSha {
        self.remote(repository);
        let mirror = self.folders.mirror(repository);
        fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        let remote = self.remotes.join(format!("{repository}.git"));
        let (from, to) = (remote.to_string_lossy(), mirror.to_string_lossy());
        let _ = git(&self.remotes, &["clone", "-q", "--bare", &from, &to]);
        CommitSha::parse(git(&mirror, &["rev-parse", "HEAD"]).trim()).unwrap()
    }

    /// A real mirror of a new remote for `name`, and the worker that cloned it.
    fn cloned(&self, name: &RepositoryName) -> Mirrors {
        self.remote(name);
        let (mirrors, announced) = self.start();
        mirrors.requests().clone_mirror(name.clone(), download());
        let _clone = one_clone(&announced);
        mirrors
    }
}

/// What a fetch of reviewed commits gives git: a refspec for each, from its SHA to its reviewed ref.
fn reviewed_refspec(number: u32, commit: &CommitSha) -> String {
    format!("+{}:refs/tsugiki/reviewed/{number}\n", commit.as_str())
}

#[test]
fn a_reviewed_commit_the_mirror_has_goes_under_its_reviewed_ref_without_asking_github() {
    let place = Place::new("reviewed-at-hand");
    let hello = named("octo-org/hello");
    let mirrors = place.cloned(&hello);
    let commit = CommitSha::parse(&place.pull_request(&hello, 1)).unwrap();
    let pr_head = PrHead {
        commit: commit.clone(),
        ..head(1)
    };
    mirrors.requests().fetch_heads(hello.clone(), vec![pr_head]);
    let fetched = place
        .folders
        .mirror(&hello)
        .join("refs/tsugiki/pull/1/head");
    wait_until(|| fetched.exists());
    // GitHub can't be reached from here on.
    fs::remove_dir_all(place.remotes.join("octo-org/hello.git")).unwrap();

    mirrors
        .requests()
        .keep_reviewed(hello.clone(), vec![reviewed(1, &commit)]);
    wait_until(|| place.reviewed_ref(&hello, 1).is_some());
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(place.reviewed_ref(&hello, 1), Some(commit));
}

#[test]
fn a_reviewed_commit_the_mirror_lacks_arrives_by_its_sha_and_its_ref_moves_with_the_review() {
    let place = Place::new("reviewed-fetched");
    let hello = named("octo-org/hello");
    let mirrors = place.cloned(&hello);
    let first = place.dropped_commit(&hello, 1);

    mirrors
        .requests()
        .keep_reviewed(hello.clone(), vec![reviewed(1, &first)]);
    wait_until(|| place.reviewed_ref(&hello, 1).is_some());
    let kept_first = place.reviewed_ref(&hello, 1);
    // The user reviews the PR again, at a later commit.
    let second = place.dropped_commit(&hello, 1);
    mirrors
        .requests()
        .keep_reviewed(hello.clone(), vec![reviewed(1, &second)]);
    wait_until(|| place.reviewed_ref(&hello, 1).as_ref() == Some(&second));
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(kept_first, Some(first));
    assert_eq!(place.reviewed_ref(&hello, 1), Some(second));
    let refs = git(
        &place.folders.mirror(&hello),
        &["for-each-ref", "--format=%(refname)", "refs/tsugiki"],
    );
    assert_eq!(refs, "refs/tsugiki/reviewed/1\n");
}

#[test]
fn a_commit_github_refuses_is_noted_beside_the_mirror_and_the_others_still_arrive() {
    let place = Place::new("reviewed-refused");
    let hello = named("octo-org/hello");
    let mirrors = place.cloned(&hello);
    let gone = CommitSha::parse(&"1".repeat(40)).unwrap();
    let there = place.dropped_commit(&hello, 2);

    let commits = vec![reviewed(1, &gone), reviewed(2, &there)];
    mirrors.requests().keep_reviewed(hello.clone(), commits);
    wait_until(|| place.reviewed_ref(&hello, 2).is_some());
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(place.reviewed_ref(&hello, 2), Some(there));
    assert_eq!(place.reviewed_ref(&hello, 1), None);
    assert_eq!(place.refused(&hello), format!("{}\n", gone.as_str()));
}

#[test]
fn a_refused_commit_isnt_asked_for_again_even_after_a_relaunch() {
    let place = Place::new("reviewed-refused-once");
    let cpython = named("python/cpython");
    place.bare_mirror(&cpython);
    let commit = head(1).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let input = fetch.input.clone();
    fail_saying(fetch, &format!("{NOT_OUR_REF} {}\n", commit.as_str()));
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let again = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);
    let (mirrors, starts) = place.start_faked();
    mirrors
        .requests()
        .keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let after_relaunch = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(input, reviewed_refspec(1, &commit));
    assert!(again.is_err(), "no second fetch");
    assert!(after_relaunch.is_err(), "no fetch after a relaunch");
}

#[test]
fn a_fetch_that_fails_for_another_reason_notes_nothing_and_tries_again_at_the_next_hand_over() {
    let place = Place::new("reviewed-failed");
    let cpython = named("python/cpython");
    place.bare_mirror(&cpython);
    let commit = head(1).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    // Words that name the commit don't make it refused.
    let full_disk = format!(
        "error: unable to write {}: No space left on device\n",
        commit.as_str()
    );
    fail_saying(starts.recv_timeout(PLENTY).unwrap(), &full_disk);
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let again = starts.recv_timeout(PLENTY).unwrap();
    let input = again.input.clone();
    again.succeed_unread();
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let once_kept = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(input, reviewed_refspec(1, &commit));
    assert!(once_kept.is_err(), "a kept commit isn't fetched again");
    assert_eq!(place.refused(&cpython), "");
}

#[test]
fn a_reviewed_commit_waits_for_its_mirror_to_be_cloned() {
    let place = Place::new("reviewed-waits");
    let cpython = named("python/cpython");
    let (mirrors, starts) = place.start_faked();

    mirrors
        .requests()
        .keep_reviewed(cpython.clone(), vec![reviewed(1, &head(1).commit)]);
    let nothing = starts.recv_timeout(A_MOMENT);
    place.bare_mirror(&cpython);
    mirrors
        .requests()
        .fetch_heads(cpython.clone(), vec![head(2)]);
    let heads = starts.recv_timeout(PLENTY).unwrap();
    let head_refspecs: Vec<String> = heads
        .refspecs()
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    heads.succeed();
    let reviewed_fetch = starts.recv_timeout(PLENTY).unwrap();
    let input = reviewed_fetch.input.clone();
    reviewed_fetch.succeed_unread();
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing.is_err(), "nothing starts without the mirror");
    assert_eq!(head_refspecs, [refspec(2)], "heads go first");
    assert_eq!(input, reviewed_refspec(1, &head(1).commit));
}

#[test]
fn a_hand_over_during_a_failed_fetch_tries_again_at_once_and_one_during_a_fetch_that_lands_asks_nothing()
 {
    let place = Place::new("reviewed-news");
    let cpython = named("python/cpython");
    place.bare_mirror(&cpython);
    let commit = head(1).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    let hand_over = || requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);

    hand_over();
    let first = starts.recv_timeout(PLENTY).unwrap();
    hand_over();
    fail_saying(
        first,
        "fatal: unable to access 'https://github.com/': Could not resolve host\n",
    );
    let second = starts.recv_timeout(PLENTY).unwrap();
    hand_over();
    second.succeed_unread();
    let third = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(
        third.is_err(),
        "the commit handed over during the fetch arrived with it"
    );
}

#[test]
fn a_refusal_that_cant_be_noted_on_disk_still_isnt_asked_for_again() {
    let place = Place::new("reviewed-refused-unnoted");
    let cpython = named("python/cpython");
    place.bare_mirror(&cpython);
    // A folder where the file goes can be neither read nor written.
    fs::create_dir_all(
        place
            .folders
            .repository(&cpython)
            .join("refused-commits.txt"),
    )
    .unwrap();
    let commit = head(1).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(fetch, &format!("{NOT_OUR_REF} {}\n", commit.as_str()));
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let again = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(again.is_err(), "no second fetch");
}

#[test]
fn a_mirror_git_cant_look_in_has_its_reviewed_commits_tried_again_at_the_next_hand_over() {
    let place = Place::new("reviewed-unreadable");
    let cpython = named("python/cpython");
    // Something is where the mirror goes, but git can't go into it. An empty folder wouldn't do: git would
    // look in the repository around it.
    let mirror = place.folders.mirror(&cpython);
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    fs::write(&mirror, "").unwrap();
    let commit = head(1).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let nothing = starts.recv_timeout(A_MOMENT);
    fs::remove_file(&mirror).unwrap();
    place.bare_mirror(&cpython);
    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &commit)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let input = fetch.input.clone();
    fetch.succeed_unread();
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing.is_err(), "nothing starts");
    assert_eq!(input, reviewed_refspec(1, &commit));
}

#[test]
fn commits_the_mirror_has_get_their_refs_first_and_the_others_are_fetched_though_that_fails() {
    let place = Place::new("reviewed-pointed");
    let cpython = named("python/cpython");
    let present = place.mirror_with_a_commit(&cpython);
    let missing = head(2).commit;
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    let commits = vec![reviewed(1, &present), reviewed(2, &missing)];

    requests.keep_reviewed(cpython.clone(), commits.clone());
    let point = starts.recv_timeout(PLENTY).unwrap();
    let (point_args, point_input) = (point.args.clone(), point.input.clone());
    fail_saying(
        point,
        "fatal: cannot lock ref 'refs/tsugiki/reviewed/1': Unable to create a lock\n",
    );
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let fetch_input = fetch.input.clone();
    fetch.succeed_unread();
    requests.keep_reviewed(cpython.clone(), commits);
    let again = starts.recv_timeout(PLENTY).unwrap();
    let again_input = again.input.clone();
    again.succeed_unread();
    mirrors.stop_cleanly(PLENTY);

    let mirror = place.folders.mirror(&cpython);
    assert_eq!(
        point_args,
        ["-C", &mirror.to_string_lossy(), "update-ref", "--stdin"]
    );
    let pointing = format!("update refs/tsugiki/reviewed/1 {}\n", present.as_str());
    assert_eq!(point_input, pointing);
    assert_eq!(fetch_input, reviewed_refspec(2, &missing));
    assert_eq!(
        again_input, pointing,
        "only the ref that failed is pointed again"
    );
}

#[test]
fn a_mirror_found_broken_while_pointing_reviewed_refs_is_cloned_again() {
    let place = Place::new("reviewed-broken-point");
    let cpython = named("python/cpython");
    let present = place.mirror_with_a_commit(&cpython);
    let (mirrors, starts) = place.start_faked();
    // The user has said where the mirror gets its objects, so a new clone doesn't wait for an answer.
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &present)]);
    let point = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(point, "fatal: bad object refs/tsugiki/pull/3/head\n");
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let clones = clone.args.iter().any(|arg| arg == "clone");
    mirrors.stop_cleanly(PLENTY);

    assert!(clones);
}

#[test]
fn a_mirror_found_broken_while_fetching_reviewed_commits_is_cloned_again_and_gets_them_all_back() {
    let place = Place::new("reviewed-broken-fetch");
    let cpython = named("python/cpython");
    place.bare_mirror(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.keep_reviewed(cpython.clone(), vec![reviewed(1, &head(1).commit)]);
    starts.recv_timeout(PLENTY).unwrap().succeed_unread();
    requests.keep_reviewed(cpython.clone(), vec![reviewed(2, &head(2).commit)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(
        fetch,
        "fatal: bad object 0123456789abcdef0123456789abcdef01234567\n",
    );
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let partial = PathBuf::from(clone.args.last().unwrap());
    fs::create_dir_all(&partial).unwrap();
    let _ = git(&partial, &["init", "-q", "--bare"]);
    clone.succeed();
    let again = starts.recv_timeout(PLENTY).unwrap();
    let input = again.input.clone();
    again.succeed_unread();
    mirrors.stop_cleanly(PLENTY);

    let both = reviewed_refspec(1, &head(1).commit) + &reviewed_refspec(2, &head(2).commit);
    assert_eq!(input, both);
}

#[test]
fn keeping_reviewed_commits_that_cant_start_waits_for_news() {
    let place = Place::new("reviewed-cant-start");
    let cpython = named("python/cpython");
    let present = place.mirror_with_a_commit(&cpython);
    let (mirrors, starts) = place.start_faked();
    drop(starts);
    let requests = mirrors.requests();
    let commits = vec![reviewed(1, &present), reviewed(2, &head(2).commit)];

    // The second hand-over is the news a stalled worker waits for, and it tries once more.
    requests.keep_reviewed(cpython.clone(), commits.clone());
    requests.keep_reviewed(cpython.clone(), commits);
    // A worker that tried again and again without news wouldn't hear the stop.
    mirrors.stop_cleanly(SOON);

    assert_eq!(place.refused(&cpython), "");
}
