//! Each Repository's Lookup checkout, which the worker moves to the PR on screen (#678).

use super::*;

/// Long enough for a git or an answer that was going to come to have come.
const A_MOMENT: Duration = Duration::from_millis(300);

impl Place {
    /// The working copy the remote for `name` came from, where the tests commit.
    fn work(&self, name: &RepositoryName) -> PathBuf {
        self.remotes.join("work").join(name.to_string())
    }

    fn bare(&self, name: &RepositoryName) -> String {
        let bare = self.remotes.join(format!("{name}.git"));
        bare.to_string_lossy().into_owned()
    }

    /// The commit `main` is at on the remote for `name`.
    fn main(&self, name: &RepositoryName) -> CommitSha {
        let commit = git(&self.work(name), &["rev-parse", "HEAD"]);
        CommitSha::parse(commit.trim()).unwrap()
    }

    /// Commits `text` to `file` on `main` on the remote for `name`. Gives the commit.
    fn commit_to_main(&self, name: &RepositoryName, file: &str, text: &str) -> CommitSha {
        let work = self.work(name);
        fs::write(work.join(file), text).unwrap();
        let _ = git(&work, &["add", file]);
        let _ = git(&work, &["commit", "-qm", file]);
        let _ = git(&work, &["push", "-q", &self.bare(name), "HEAD:main"]);
        self.main(name)
    }

    /// A PR `number` on the remote for `name` that adds `file` on top of `main`. Gives its head.
    fn pull_request_adding(&self, name: &RepositoryName, number: u32, file: &str) -> CommitSha {
        let work = self.work(name);
        let path = work.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("PR {number}\n")).unwrap();
        let _ = git(&work, &["add", file]);
        let _ = git(&work, &["commit", "-qm", &format!("PR {number}")]);
        let head = self.main(name);
        let refspec = format!("HEAD:refs/pull/{number}/head");
        let _ = git(&work, &["push", "-q", &self.bare(name), &refspec]);
        let _ = git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
        head
    }

    /// The worker with the real git, once it has cloned `name`'s mirror and fetched the heads of the PRs
    /// `heads` gives by number.
    fn mirror_with(&self, name: &RepositoryName, heads: &[(u32, &CommitSha)]) -> Mirrors {
        let (mirrors, announced) = self.start();
        mirrors.requests().clone_mirror(name.clone(), download());
        let _clone = one_clone(&announced);
        let heads: Vec<PrHead> = (heads.iter())
            .map(|(number, commit)| PrHead {
                commit: (*commit).clone(),
                ..head(*number)
            })
            .collect();
        let count = heads.len();
        mirrors.requests().fetch_heads(name.clone(), heads);
        let mirror = self.folders.mirror(name);
        wait_until(|| {
            let refs = git(&mirror, &["for-each-ref", "refs/tsugiki/pull"]);
            refs.lines().count() == count
        });
        mirrors
    }

    /// Where the PR on screen wants the Lookup checkout of `name`, and where the worker says it is then.
    fn look_up(&self, mirrors: &Mirrors, name: &RepositoryName, at: LookupAt) -> LookupCheckout {
        mirrors.requests().look_up(name.clone(), at);
        mirrors.requests().lookup_checkout(name.clone()).unwrap()
    }
}

/// A PR's two ends, as the fake git sees them: commits nothing holds.
fn fake_at(number: u32) -> LookupAt {
    LookupAt {
        base: head(number).commit,
        head: head(number + 50).commit,
    }
}

/// The commit a checkout's HEAD is at.
fn checked_out(folder: &Path) -> String {
    git(folder, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// The git command `start` runs, after its options.
fn command(start: &FakeStart) -> String {
    let commands = ["worktree", "reset", "checkout"];
    let found = start
        .args
        .iter()
        .find(|arg| commands.contains(&arg.as_str()));
    found.unwrap().clone()
}

/// Ends each git of the move under way as a real one would, and gives the commands it ran and the commit
/// its reset went to. Adding the worktree leaves the file git leaves in its folder.
fn end_move(starts: &Receiver<FakeStart>, folder: &Path) -> (Vec<String>, String) {
    let mut commands = Vec::new();
    loop {
        let start = starts.recv_timeout(PLENTY).unwrap();
        let command = command(&start);
        commands.push(command.clone());
        match command.as_str() {
            "worktree" => {
                fs::create_dir_all(folder).unwrap();
                fs::write(folder.join(".git"), "gitdir: elsewhere\n").unwrap();
                start.succeed();
            }
            "reset" => {
                let base = start.args.last().unwrap().clone();
                start.succeed();
                return (commands, base);
            }
            other => panic!("a move doesn't run git {other}"),
        }
    }
}

#[test]
fn the_checkout_is_made_at_a_prs_base_with_its_added_files_and_the_next_pr_moves_it() {
    let place = Place::new("lookup-moves");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let first_base = place.main(&hello);
    let one = place.pull_request_adding(&hello, 1, "NEWS");
    let second_base = place.commit_to_main(&hello, "README.rst", "Second\n");
    let two = place.pull_request_adding(&hello, 2, "Lib/new module.py");
    let mirrors = place.mirror_with(&hello, &[(1, &one), (2, &two)]);

    let at_one = LookupAt {
        base: first_base.clone(),
        head: one,
    };
    let first = place.look_up(&mirrors, &hello, at_one.clone());
    let first_commit = checked_out(&first.folder);
    let first_news = fs::read_to_string(first.folder.join("NEWS")).unwrap();
    let at_two = LookupAt {
        base: second_base.clone(),
        head: two,
    };
    let second = place.look_up(&mirrors, &hello, at_two.clone());
    let second_commit = checked_out(&second.folder);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(first.folder, place.folders.lookup(&hello));
    assert_eq!(first.at, at_one);
    assert_eq!(first_commit, first_base.as_str());
    assert_eq!(first_news, "PR 1\n");
    assert_eq!(second.at, at_two);
    assert_eq!(second_commit, second_base.as_str());
    assert!(
        !second.folder.join("NEWS").exists(),
        "the next move removes a PR's added files"
    );
    let added = fs::read_to_string(second.folder.join("Lib/new module.py")).unwrap();
    assert_eq!(added, "PR 2\n");
    let readme = fs::read_to_string(second.folder.join("README.rst")).unwrap();
    assert_eq!(readme, "Second\n");
}

#[test]
fn the_mirrors_gc_keeps_the_commit_the_checkout_is_on() {
    let place = Place::new("lookup-gc");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let one = place.pull_request_adding(&hello, 1, "NEWS");
    let mirrors = place.mirror_with(&hello, &[(1, &one)]);
    // The PR's own commit, which only its ref holds.
    let at = LookupAt {
        base: one.clone(),
        head: one.clone(),
    };
    let checkout = place.look_up(&mirrors, &hello, at);
    mirrors.stop_cleanly(PLENTY);

    // PR 1 has left the work, so its ref goes, and gc prunes what nothing reaches.
    let mirror = place.folders.mirror(&hello);
    let _ = git(&mirror, &["update-ref", "-d", "refs/tsugiki/pull/1/head"]);
    let _ = git(&mirror, &["gc", "--quiet", "--prune=now"]);

    let kind = git(&mirror, &["cat-file", "-t", one.as_str()]);
    assert_eq!(kind.trim(), "commit");
    assert_eq!(checked_out(&checkout.folder), one.as_str());
}

#[test]
fn with_no_network_the_checkout_moves_to_a_base_the_mirror_holds() {
    let place = Place::new("lookup-offline");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let base = place.main(&hello);
    let one = place.pull_request_adding(&hello, 1, "NEWS");
    let mirrors = place.mirror_with(&hello, &[(1, &one)]);

    mirrors.requests().follow(&Status::Offline);
    let gone = place.remotes.with_file_name("remotes-gone");
    fs::rename(&place.remotes, &gone).unwrap();
    let at = LookupAt {
        base: base.clone(),
        head: one,
    };
    let checkout = place.look_up(&mirrors, &hello, at.clone());
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(checkout.at, at);
    assert_eq!(checked_out(&checkout.folder), base.as_str());
    assert!(checkout.folder.join("NEWS").is_file());
}

#[test]
fn removing_the_repositorys_folder_removes_the_checkout_and_gits_note_of_it() {
    let place = Place::new("lookup-removed");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let base = place.main(&hello);
    let one = place.pull_request_adding(&hello, 1, "NEWS");
    let mirrors = place.mirror_with(&hello, &[(1, &one)]);
    let at = LookupAt { base, head: one };
    let checkout = place.look_up(&mirrors, &hello, at);
    mirrors.stop_cleanly(PLENTY);
    let note = fs::read_to_string(checkout.folder.join(".git")).unwrap();
    let note = Path::new(note.trim().strip_prefix("gitdir: ").unwrap());
    let note = note.canonicalize().unwrap();
    let repository = place.folders.repository(&hello).canonicalize().unwrap();

    fs::remove_dir_all(&repository).unwrap();

    // git notes the worktree in the mirror, which is in the Repository's folder too.
    assert!(
        note.starts_with(&repository),
        "{note:?} is outside {repository:?}"
    );
    assert!(!note.exists());
    assert!(!checkout.folder.exists());
}

#[test]
fn three_prs_opened_in_a_row_end_at_the_thirds_base_in_two_moves() {
    let place = Place::new("lookup-in-a-row");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let folder = place.folders.lookup(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    for number in 1..=3 {
        requests.look_up(cpython.clone(), fake_at(number));
    }
    let first = end_move(&starts, &folder);
    let second = end_move(&starts, &folder);
    let checkout = requests.lookup_checkout(cpython.clone());
    let more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    let base = |number: u32| fake_at(number).base.as_str().to_owned();
    assert_eq!(first, (vec!["worktree".into(), "reset".into()], base(1)));
    assert_eq!(
        second,
        (vec!["reset".into()], base(3)),
        "a checkout that is there is only reset"
    );
    assert!(more.is_err(), "{:?}", more.map(|start| start.args));
    assert_eq!(checkout.map(|checkout| checkout.at), Some(fake_at(3)));
}

#[test]
fn a_lookup_asked_during_a_move_waits_and_reads_the_commit_it_moved_to() {
    let place = Place::new("lookup-waits");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let folder = place.folders.lookup(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.look_up(cpython.clone(), fake_at(1));
    let make = starts.recv_timeout(PLENTY).unwrap();
    let reader = thread::spawn({
        let (requests, cpython) = (requests.clone(), cpython.clone());
        move || requests.lookup_checkout(cpython)
    });
    // The same PR again asks for nothing more.
    requests.look_up(cpython.clone(), fake_at(1));
    thread::sleep(A_MOMENT);
    let waited = !reader.is_finished();
    make.succeed();
    let (commands, _base) = end_move(&starts, &folder);
    let read = reader.join().unwrap();
    let more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(waited);
    assert_eq!(commands, ["reset"]);
    assert_eq!(
        read,
        Some(LookupCheckout {
            folder,
            at: fake_at(1)
        })
    );
    assert!(more.is_err());
}

/// Asks where `repository`'s Lookup checkout is on a thread of its own, as the answer may wait for a move.
fn ask(requests: &MirrorRequests, repository: &RepositoryName) -> Receiver<Option<LookupCheckout>> {
    let (answering, answer) = channel();
    let (requests, repository) = (requests.clone(), repository.clone());
    drop(thread::spawn(move || {
        drop(answering.send(requests.lookup_checkout(repository)));
    }));
    answer
}

#[test]
fn a_failed_move_leaves_no_checkout_and_only_a_reader_tries_it_again_afresh() {
    let place = Place::new("lookup-failed");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let folder = place.folders.lookup(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.look_up(cpython.clone(), fake_at(1));
    let make = starts.recv_timeout(PLENTY).unwrap();
    let waiting = ask(&requests, &cpython);
    thread::sleep(A_MOMENT);
    // git got as far as the file in the folder.
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(".git"), "gitdir: elsewhere\n").unwrap();
    fail_saying(make, "fatal: could not create work tree dir\n");
    let failed = waiting.recv_timeout(PLENTY).unwrap();
    // The Files tab reads the same PR again.
    requests.look_up(cpython.clone(), fake_at(1));
    let tried_again = starts.recv_timeout(A_MOMENT);
    let retried = ask(&requests, &cpython);
    let (commands, _base) = end_move(&starts, &folder);
    let moved = retried.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(failed, None);
    assert!(
        tried_again.is_err(),
        "{:?}",
        tried_again.map(|start| start.args)
    );
    assert_eq!(commands, ["worktree", "reset"]);
    assert_eq!(moved.map(|checkout| checkout.at), Some(fake_at(1)));
}

#[test]
fn a_checkout_git_has_lost_is_made_afresh_in_the_same_move() {
    let place = Place::new("lookup-lost");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let folder = place.folders.lookup(&cpython);
    // Left by the last launch, in a mirror cloned again since.
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(".git"), "gitdir: gone\n").unwrap();
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.look_up(cpython.clone(), fake_at(1));
    let reset = starts.recv_timeout(PLENTY).unwrap();
    let first = command(&reset);
    fail_saying(reset, "fatal: not a git repository: gone\n");
    let (commands, _base) = end_move(&starts, &folder);
    let checkout = requests.lookup_checkout(cpython.clone());
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(first, "reset");
    assert_eq!(commands, ["worktree", "reset"]);
    assert_eq!(checkout.map(|checkout| checkout.at), Some(fake_at(1)));
}

#[test]
fn a_reset_that_fails_in_the_checkout_the_move_made_ends_the_move() {
    let place = Place::new("lookup-reset-fails");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.look_up(cpython.clone(), fake_at(1));
    starts.recv_timeout(PLENTY).unwrap().succeed();
    let reset = starts.recv_timeout(PLENTY).unwrap();
    let waiting = ask(&requests, &cpython);
    thread::sleep(A_MOMENT);
    fail_saying(reset, "fatal: Could not reset index file to revision\n");
    let checkout = waiting.recv_timeout(PLENTY).unwrap();
    let more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(checkout, None);
    assert!(more.is_err(), "{:?}", more.map(|start| start.args));
}

#[test]
fn a_move_runs_no_hooks_and_no_filter_that_asks_the_network() {
    let place = Place::new("lookup-quiet");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();

    mirrors.requests().look_up(cpython.clone(), fake_at(1));
    let make = starts.recv_timeout(PLENTY).unwrap();
    let args = make.args.clone();
    mirrors.stop_cleanly(PLENTY);

    let hooks = place.folders.mirror(&cpython).join("hooks");
    let hooks = format!("core.hooksPath={}", hooks.display());
    let config: Vec<&str> = (args.chunks(2))
        .take_while(|pair| pair[0] == "-c")
        .map(|pair| pair[1].as_str())
        .collect();
    let expected = [
        "filter.lfs.smudge=",
        "filter.lfs.process=",
        "filter.lfs.required=false",
        &hooks,
    ];
    assert_eq!(config, expected);
}

#[test]
fn without_its_mirror_a_lookup_hears_at_once_there_is_no_checkout() {
    let place = Place::new("lookup-no-mirror");
    let cpython = named("python/cpython");
    let (mirrors, starts) = place.start_faked();

    mirrors.requests().look_up(cpython.clone(), fake_at(1));
    let checkout = mirrors.requests().lookup_checkout(cpython);
    let started = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(checkout, None);
    assert!(started.is_err());
}

#[test]
fn short_of_disk_a_checkout_isnt_made() {
    let place = Place::new("lookup-disk-short");
    let [cpython, hello] = [named("python/cpython"), named("octo-org/hello")];
    place.mirrored(&cpython);
    let gib = 1 << 30;
    let remotes =
        (place.clone_source()).on_disk(Box::new(move |_| Ok(5 * gib)), Box::new(move |_| Ok(gib)));
    let (_versions, answers) = channel();
    let announce = Announce::cloning_only(Box::new(|_| {}));
    let (mirrors, starts) = place.start_faked_from(remotes, announce, answers);

    // The clone finds the disk short.
    mirrors.requests().clone_mirror(hello, download());
    mirrors.requests().look_up(cpython.clone(), fake_at(1));
    let checkout = mirrors.requests().lookup_checkout(cpython);
    let started = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(checkout, None);
    assert!(started.is_err(), "{:?}", started.map(|start| start.args));
}

#[test]
fn a_move_that_cant_clear_the_folder_or_start_git_leaves_no_checkout() {
    let place = Place::new("lookup-cant");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let folder = place.folders.lookup(&cpython);
    fs::write(&folder, "a file where the folder goes").unwrap();
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();

    requests.look_up(cpython.clone(), fake_at(1));
    let uncleared = requests.lookup_checkout(cpython.clone());
    let started = starts.recv_timeout(A_MOMENT);
    fs::remove_file(&folder).unwrap();
    drop(starts);
    requests.look_up(cpython.clone(), fake_at(1));
    let unstarted = requests.lookup_checkout(cpython.clone());
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(uncleared, None);
    assert!(started.is_err(), "{:?}", started.map(|start| start.args));
    assert_eq!(unstarted, None);
}

#[test]
fn a_lookup_asked_where_git_is_missing_hears_at_once_there_is_none() {
    let place = Place::new("lookup-no-git");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (versions, answers) = channel();
    versions.send(None).unwrap();
    let announce = Announce::cloning_only(Box::new(|_| {}));
    let (mirrors, _starts) = place.start_faked_from(place.clone_source(), announce, answers);

    mirrors.requests().look_up(cpython.clone(), fake_at(1));
    let checkout = mirrors.requests().lookup_checkout(cpython);
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(checkout, None);
}

#[test]
fn quitting_stops_a_move_under_way() {
    let place = Place::new("lookup-quit");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();

    mirrors.requests().look_up(cpython, fake_at(1));
    let make = starts.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(make.stopped.recv_timeout(PLENTY), Ok(()));
}
