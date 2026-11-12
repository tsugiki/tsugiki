//! Each mirror's upkeep, and a broken mirror cloned again (#174).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

const HOUR: Duration = Duration::from_secs(60 * 60);

/// Long enough for a git that was going to start to have started.
const A_MOMENT: Duration = Duration::from_millis(300);

impl Place {
    /// The worker with the fake git and each mirror's upkeep on `schedule`, each start of it, and what it
    /// says of each clone.
    fn start_keeping(
        &self,
        schedule: Schedule,
    ) -> (Mirrors, Receiver<FakeStart>, Receiver<Option<Cloning>>) {
        let (announcing, announced) = channel();
        let (_versions, answers) = channel();
        let remotes = self.clone_source().patient(PLENTY).keeping(schedule);
        let (mirrors, starts) = self.start_faked_from(remotes, cloning_only(announcing), answers);
        (mirrors, starts, announced)
    }

    /// The worker with the real git and each mirror's upkeep on the usual schedule.
    fn start_live_keeping(&self) -> (Mirrors, Receiver<Option<Cloning>>) {
        let (announcing, announced) = channel();
        let mirrors = Mirrors::with_git(
            Box::new(LiveGit),
            Ok(self.folders.clone()),
            self.clone_source().keeping(Schedule::DAILY_AND_WEEKLY),
            cloning_only(announcing),
        );
        (mirrors, announced)
    }

    /// Notes `repository`'s mirror as maintained and collected at those times, as Tsugiki notes it.
    fn upkept(&self, repository: &RepositoryName, maintained: SystemTime, collected: SystemTime) {
        let seconds = |time: SystemTime| time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let text = format!(
            "maintained = {}\ncollected = {}\n",
            seconds(maintained),
            seconds(collected)
        );
        fs::write(self.upkeep_file(repository), text).unwrap();
    }

    fn upkeep_file(&self, repository: &RepositoryName) -> PathBuf {
        self.folders.repository(repository).join("upkeep.toml")
    }

    /// The time the file gives for `repository`'s mirror's `key` (`maintained` or `collected`).
    fn upkept_at(&self, repository: &RepositoryName, key: &str) -> SystemTime {
        let text = fs::read_to_string(self.upkeep_file(repository)).unwrap();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap();
        let seconds = document[key].as_integer().unwrap();
        UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).unwrap())
    }

    /// Whether the file gives a time within a minute of now for `key`. The worker writes the file over, so
    /// a look while it writes can find it empty, which says no.
    fn upkept_just_now(&self, repository: &RepositoryName, key: &str) -> bool {
        let text = fs::read_to_string(self.upkeep_file(repository)).unwrap_or_default();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap_or_default();
        let seconds = document.get(key).and_then(toml_edit::Item::as_integer);
        seconds
            .and_then(|seconds| u64::try_from(seconds).ok())
            .is_some_and(|seconds| just_now(UNIX_EPOCH + Duration::from_secs(seconds)))
    }

    /// The PR refs in `repository`'s real mirror.
    fn pull_refs(&self, repository: &RepositoryName) -> String {
        let mirror = self.folders.mirror(repository);
        git(
            &mirror,
            &["for-each-ref", "--format=%(refname)", "refs/tsugiki"],
        )
    }
}

fn ago(time: Duration) -> SystemTime {
    SystemTime::now() - time
}

/// Whether `time` is within a minute of now, as the time of a note Tsugiki made during a test is.
fn just_now(time: SystemTime) -> bool {
    SystemTime::now().duration_since(time).unwrap_or_default() < Duration::from_secs(60)
}

#[test]
fn a_mirror_that_never_had_upkeep_gets_gc_at_once_and_notes_when() {
    let place = Place::new("upkeep-never");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    let args = gc.args.clone();
    gc.succeed();
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    let mirror = place.folders.mirror(&cpython);
    assert_eq!(args, ["-C", &mirror.to_string_lossy(), "gc", "--quiet"]);
    assert!(
        nothing_more.is_err(),
        "one gc stands in for the day's maintenance"
    );
    assert!(just_now(place.upkept_at(&cpython, "maintained")));
    assert!(just_now(place.upkept_at(&cpython, "collected")));
}

#[test]
fn a_mirror_maintained_over_a_day_ago_gets_maintenance_and_keeps_its_gc_time() {
    let place = Place::new("upkeep-daily");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let collected =
        UNIX_EPOCH + Duration::from_secs(ago(HOUR).duration_since(UNIX_EPOCH).unwrap().as_secs());
    place.upkept(&cpython, ago(HOUR * 25), collected);
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let maintenance = starts.recv_timeout(PLENTY).unwrap();
    let args = maintenance.args.clone();
    maintenance.succeed();
    mirrors.stop_cleanly(PLENTY);

    let mirror = place.folders.mirror(&cpython);
    let incremental = [
        "maintenance",
        "run",
        "--quiet",
        "--task=commit-graph",
        "--task=loose-objects",
        "--task=incremental-repack",
    ];
    assert_eq!(args[..2], ["-C", &*mirror.to_string_lossy()]);
    assert_eq!(args[2..], incremental);
    assert!(just_now(place.upkept_at(&cpython, "maintained")));
    assert_eq!(place.upkept_at(&cpython, "collected"), collected);
}

#[test]
fn a_mirror_kept_lately_waits_for_its_turn_and_the_worker_wakes_for_it() {
    let place = Place::new("upkeep-wait");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    place.upkept(&cpython, SystemTime::now(), SystemTime::now());
    let schedule = Schedule {
        maintain: Duration::from_secs(2),
        collect: HOUR,
    };
    let (mirrors, starts, _announced) = place.start_keeping(schedule);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let early = starts.recv_timeout(A_MOMENT);
    let on_time = starts.recv_timeout(PLENTY).unwrap();
    let args = on_time.args.clone();
    on_time.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert!(early.is_err(), "nothing is due yet");
    assert_eq!(args[2..4], ["maintenance", "run"]);
}

#[test]
fn refs_of_prs_the_work_no_longer_reaches_go_before_maintenance_but_opened_ones_stay() {
    let place = Place::new("upkeep-prune");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    place.upkept(&cpython, ago(HOUR * 25), SystemTime::now());
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);
    let requests = mirrors.requests();

    // All before the worker hears of the Repository's clone, so no upkeep starts before the hand-overs.
    // PR 7 opened with its head in the mirror, PR 5 without.
    requests.opened(cpython.clone(), 7);
    requests.fetch_first(cpython.clone(), head(5));
    starts.recv_timeout(PLENTY).unwrap().succeed();
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2), head(3)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let refs = fetch.refs.clone();
    fetch.succeed();
    // PR 3 leaves the work, and its head, fetched already, starts no fetch.
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    requests.opened(cpython.clone(), 8);
    // The refs the mirror keeps when the upkeep looks.
    let listing: String = [1, 2, 3, 5, 7, 8]
        .map(|number| {
            format!(
                "{} refs/tsugiki/pull/{number}/head\n",
                head(number).commit.as_str()
            )
        })
        .concat();
    refs.send(listing).unwrap();
    requests.clone_mirror(cpython.clone(), download());
    let prune = starts.recv_timeout(PLENTY).unwrap();
    let (prune_args, deletes) = (prune.args.clone(), prune.input.clone());
    prune.succeed();
    let maintenance = starts.recv_timeout(PLENTY).unwrap();
    let maintenance_args = maintenance.args.clone();
    maintenance.succeed();
    // PR 3 comes back in reach, so its head is fetched again.
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2), head(3)]);
    let again = starts.recv_timeout(PLENTY).unwrap();
    let refspecs: Vec<String> = again
        .refspecs()
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    again.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(prune_args[2..], ["update-ref", "--stdin"]);
    assert_eq!(deletes, "delete refs/tsugiki/pull/3/head\n");
    assert_eq!(maintenance_args[2..4], ["maintenance", "run"]);
    assert_eq!(refspecs, [refspec(3)]);
}

#[test]
fn refs_that_cant_be_deleted_leave_the_maintenance_to_run() {
    let place = Place::new("upkeep-prune-fails");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    place.upkept(&cpython, ago(HOUR * 25), SystemTime::now());
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    requests.clone_mirror(cpython.clone(), download());
    let listing = format!("{} refs/tsugiki/pull/1/head\n", head(1).commit.as_str());
    fetch.refs.send(listing.clone()).unwrap();
    fetch
        .refs
        .send(format!(
            "{listing}{} refs/tsugiki/pull/2/head\n",
            head(2).commit.as_str()
        ))
        .unwrap();
    fetch.end(Outcome::Succeeded);
    let prune = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(
        prune,
        "fatal: Unable to create 'packed-refs.lock': File exists.\n",
    );
    let maintenance = starts.recv_timeout(PLENTY).unwrap();
    let args = maintenance.args.clone();
    maintenance.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(args[2..4], ["maintenance", "run"]);
    assert!(place.folders.mirror(&cpython).exists());
}

#[test]
fn an_upkeep_that_fails_waits_for_its_next_turn_and_leaves_the_mirror() {
    let place = Place::new("upkeep-fails");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(
        gc,
        "fatal: gc is already running on machine 'here' pid 1 (use --force if not)\n",
    );
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing_more.is_err());
    assert!(place.folders.mirror(&cpython).exists());
    assert!(just_now(place.upkept_at(&cpython, "collected")));
}

#[test]
fn a_mirror_a_fetch_finds_broken_is_cloned_again_and_gets_its_heads_back() {
    let place = Place::new("broken-fetch");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);
    // Its upkeep is done for now, so only the clone and the fetches start.
    place.upkept(&cpython, SystemTime::now(), SystemTime::now());
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().succeed();
    requests.fetch_heads(cpython.clone(), vec![head(1), head(2)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    // git's hint after its reason doesn't hide the reason.
    fail_saying(
        fetch,
        "fatal: bad object 0123456789abcdef0123456789abcdef01234567\nhint: run git fsck\n",
    );
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let cloning = announced.recv_timeout(PLENTY).unwrap();
    let broken = place.folders.repository(&cpython).join("mirror.broken");
    let set_aside = broken.exists();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    let clone_args = clone.args.clone();
    clone.succeed();
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let refspecs: Vec<String> = fetch
        .refspecs()
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    fetch.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert!(set_aside, "the broken mirror waits beside the clone");
    assert_eq!(
        cloning.map(|cloning| cloning.repository),
        Some(cpython.clone())
    );
    assert!(
        clone_args
            .windows(2)
            .any(|pair| pair == ["--config", "maintenance.auto=false"]),
        "{clone_args:?}"
    );
    assert!(!broken.exists(), "the new clone deletes the broken mirror");
    assert!(place.folders.mirror(&cpython).exists());
    assert_eq!(refspecs, [refspec(1), refspec(2)]);
    assert!(just_now(place.upkept_at(&cpython, "collected")));
}

#[test]
fn a_mirror_an_upkeep_finds_broken_is_cloned_again() {
    let place = Place::new("broken-upkeep");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    // A broken mirror that an earlier replacement left behind.
    let broken = place.folders.repository(&cpython).join("mirror.broken");
    fs::create_dir_all(&broken).unwrap();
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(
        gc,
        "error: loose object 0123 (stored in objects/01/23) is corrupt\n",
    );
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let is_clone = clone.args.first().is_some_and(|arg| arg == "clone");
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(is_clone);
    assert!(nothing_more.is_err(), "a new clone needs no upkeep yet");
    assert!(!broken.exists());
}

#[test]
fn a_broken_mirror_that_cant_be_set_aside_stays_and_its_fetches_wait_for_news() {
    let place = Place::new("broken-stays");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    // A file where the broken mirror would go, which the worker can't delete as a folder.
    fs::write(place.folders.repository(&cpython).join("mirror.broken"), "").unwrap();
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(fetch, "fatal: bad object HEAD\n");
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing_more.is_err(), "no clone, and no fetch until news");
    assert!(place.folders.mirror(&cpython).exists());
}

#[test]
fn a_broken_mirror_whose_upkeep_failed_and_cant_be_set_aside_waits_for_its_next_turn() {
    let place = Place::new("broken-upkeep-stays");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    fs::write(place.folders.repository(&cpython).join("mirror.broken"), "").unwrap();
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(gc, "fatal: bad object HEAD\n");
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing_more.is_err());
    assert!(just_now(place.upkept_at(&cpython, "collected")));
}

#[test]
fn git_says_a_mirror_is_broken_in_its_own_words_and_not_for_the_network_or_the_disk() {
    for words in [
        "fatal: bad object refs/tsugiki/pull/1/head",
        "error: bad tree object 0123",
        "fatal: loose object 0123 (stored in objects/01/23) is corrupt",
        "error: index file corrupt",
        "fatal: missing blob object '0123'",
        "error: missing tree 0123",
        "fatal: missing commit 0123",
        "error: missing object 0123",
        "fatal: not a git repository: 'D:/data/mirror.git'",
    ] {
        assert!(says_broken(words), "{words}");
    }
    for words in [
        "fatal: unable to access 'https://github.com/python/cpython/': Could not resolve host: github.com",
        "fatal: write error: No space left on device",
        "fatal: the remote end hung up unexpectedly",
        "fatal: unable to access 'https://github.com/acme/corrupt-detector.git/': Could not resolve host: github.com",
        "",
    ] {
        assert!(!says_broken(words), "{words}");
    }
}

#[test]
fn a_file_tsugiki_cant_read_counts_as_no_upkeep_yet() {
    let place = Place::new("upkeep-garbled");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    fs::write(place.upkeep_file(&cpython), "maintained = [\n").unwrap();
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    let args = gc.args.clone();
    gc.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(args[2..], ["gc", "--quiet"]);
}

#[test]
fn without_upkeep_tsugiki_leaves_mirrors_as_they_are() {
    let place = Place::new("upkeep-off");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let nothing = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing.is_err());
    assert!(!place.upkeep_file(&cpython).exists());
}

#[test]
fn a_real_mirror_drops_the_refs_of_prs_gone_from_the_work_and_gets_maintenance_and_gc() {
    let place = Place::new("upkeep-real");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let heads: Vec<PrHead> = [1, 2]
        .map(|number| PrHead {
            commit: CommitSha::parse(&place.pull_request(&hello, number)).unwrap(),
            ..head(number)
        })
        .into();
    let (mirrors, announced) = place.start_live_keeping();
    mirrors.requests().clone_mirror(hello.clone(), download());
    let _clone = one_clone(&announced);
    mirrors.requests().fetch_heads(hello.clone(), heads.clone());
    wait_until(|| place.pull_refs(&hello).lines().count() == 2);
    mirrors.stop_cleanly(PLENTY);
    let config = git(
        &place.folders.mirror(&hello),
        &["config", "maintenance.auto"],
    );

    // A day later, PR 2 has left the work.
    let collected =
        UNIX_EPOCH + Duration::from_secs(ago(HOUR).duration_since(UNIX_EPOCH).unwrap().as_secs());
    place.upkept(&hello, ago(HOUR * 25), collected);
    let (mirrors, _announced) = place.start_live_keeping();
    mirrors
        .requests()
        .fetch_heads(hello.clone(), heads[..1].to_vec());
    mirrors.requests().clone_mirror(hello.clone(), download());
    wait_until(|| place.upkept_just_now(&hello, "maintained"));
    mirrors.stop_cleanly(PLENTY);
    let after_maintenance = place.pull_refs(&hello);

    // A week later.
    place.upkept(&hello, SystemTime::now(), ago(HOUR * 24 * 8));
    let (mirrors, _announced) = place.start_live_keeping();
    mirrors.requests().clone_mirror(hello.clone(), download());
    wait_until(|| place.upkept_just_now(&hello, "collected"));
    mirrors.stop_cleanly(PLENTY);
    let mirror = place.folders.mirror(&hello);
    let fsck = LiveGit.read(&[
        OsStr::new("-C"),
        mirror.as_os_str(),
        OsStr::new("fsck"),
        OsStr::new("--no-progress"),
    ]);

    assert_eq!(config.trim(), "false");
    assert_eq!(after_maintenance, "refs/tsugiki/pull/1/head\n");
    assert!(fsck.is_ok(), "{fsck:?}");
    assert_eq!(place.pull_refs(&hello), "refs/tsugiki/pull/1/head\n");
}

#[test]
fn upkeep_leaves_the_reviewed_ref_of_a_pr_gone_from_the_work() {
    let place = Place::new("upkeep-reviewed");
    let hello = named("octo-org/hello");
    place.remote(&hello);
    let heads: Vec<PrHead> = [1, 2]
        .map(|number| PrHead {
            commit: CommitSha::parse(&place.pull_request(&hello, number)).unwrap(),
            ..head(number)
        })
        .into();
    let (mirrors, announced) = place.start_live_keeping();
    mirrors.requests().clone_mirror(hello.clone(), download());
    let _clone = one_clone(&announced);
    mirrors.requests().fetch_heads(hello.clone(), heads.clone());
    let review = reviewed(2, &heads[1].commit);
    mirrors
        .requests()
        .keep_reviewed(hello.clone(), vec![review]);
    wait_until(|| place.pull_refs(&hello).lines().count() == 3);
    mirrors.stop_cleanly(PLENTY);

    // A day later, PR 2 has left the work.
    place.upkept(&hello, ago(HOUR * 25), SystemTime::now());
    let (mirrors, _announced) = place.start_live_keeping();
    mirrors
        .requests()
        .fetch_heads(hello.clone(), heads[..1].to_vec());
    mirrors.requests().clone_mirror(hello.clone(), download());
    wait_until(|| place.upkept_just_now(&hello, "maintained"));
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(
        place.pull_refs(&hello),
        "refs/tsugiki/pull/1/head\nrefs/tsugiki/reviewed/2\n"
    );
}

#[test]
fn refs_that_cant_be_read_leave_the_maintenance_to_run() {
    let place = Place::new("upkeep-unread");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    place.upkept(&cpython, ago(HOUR * 25), SystemTime::now());
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    starts.recv_timeout(PLENTY).unwrap().succeed();
    // The fake git gives no refs for the upkeep to read.
    requests.clone_mirror(cpython.clone(), download());
    let maintenance = starts.recv_timeout(PLENTY).unwrap();
    let args = maintenance.args.clone();
    maintenance.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(args[2..4], ["maintenance", "run"]);
}

#[test]
fn an_upkeep_that_cant_start_waits_for_its_next_turn() {
    let place = Place::new("upkeep-cant-start");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);
    // A test that has stopped listening has a git that can't start.
    drop(starts);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    wait_until(|| place.upkeep_file(&cpython).exists());
    mirrors.stop_cleanly(PLENTY);

    assert!(just_now(place.upkept_at(&cpython, "collected")));
}

#[test]
fn an_upkeep_file_that_cant_be_written_leaves_a_trace_and_the_worker_goes_by_its_own_copy() {
    let place = Place::new("upkeep-unwritten");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    // A folder where the file goes, which can't be written as a file.
    fs::create_dir_all(place.upkeep_file(&cpython)).unwrap();
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    starts.recv_timeout(PLENTY).unwrap().succeed();
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing_more.is_err());
}

#[test]
fn a_broken_mirror_the_new_clone_cant_delete_leaves_a_trace() {
    let place = Place::new("broken-undeleted");
    let cpython = named("python/cpython");
    // A file where the broken mirror waits, which deleting a folder can't remove.
    let broken = place.folders.repository(&cpython).join("mirror.broken");
    fs::create_dir_all(broken.parent().unwrap()).unwrap();
    fs::write(&broken, "").unwrap();
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let clone = starts.recv_timeout(PLENTY).unwrap();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(nothing_more.is_err(), "a new clone needs no upkeep yet");
    assert!(broken.exists());
    assert!(place.folders.mirror(&cpython).exists());
}

#[test]
fn quitting_stops_an_upkeep_under_way() {
    let place = Place::new("upkeep-stop");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, _announced) = place.start_keeping(Schedule::DAILY_AND_WEEKLY);

    mirrors.requests().clone_mirror(cpython, download());
    let gc = starts.recv_timeout(PLENTY).unwrap();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(gc.stopped.recv_timeout(PLENTY), Ok(()));
}

#[test]
fn a_mirror_found_broken_again_since_launch_stays_and_its_fetches_wait_for_news() {
    let place = Place::new("broken-twice");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    fail_saying(
        starts.recv_timeout(PLENTY).unwrap(),
        "fatal: bad object HEAD\n",
    );
    let clone = starts.recv_timeout(PLENTY).unwrap();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    fail_saying(
        starts.recv_timeout(PLENTY).unwrap(),
        "fatal: bad object HEAD\n",
    );
    let nothing_more = starts.recv_timeout(A_MOMENT);
    mirrors.stop_cleanly(PLENTY);

    assert!(
        nothing_more.is_err(),
        "no second clone, and no fetch until news"
    );
    assert!(place.folders.mirror(&cpython).exists());
}

#[test]
fn a_head_due_another_fetch_waits_for_the_new_clone_too() {
    let place = Place::new("broken-behind");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts) = place.start_faked();
    let requests = mirrors.requests();
    requests.clone_mirror(cpython.clone(), download());

    requests.fetch_heads(cpython.clone(), vec![head(1)]);
    // GitHub hadn't moved the ref yet, so PR 1's head is due another fetch.
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older(1)]);
    // A hand-over without PR 1 leaves it due another fetch.
    requests.fetch_heads(cpython.clone(), vec![head(2)]);
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    fail_saying(fetch, "fatal: bad object HEAD\n");
    let clone = starts.recv_timeout(PLENTY).unwrap();
    fs::create_dir_all(clone.args.last().unwrap()).unwrap();
    clone.succeed();
    let fetch = starts.recv_timeout(PLENTY).unwrap();
    let refspecs: Vec<String> = fetch
        .refspecs()
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
    fetch.succeed();
    mirrors.stop_cleanly(PLENTY);

    assert_eq!(refspecs, [refspec(1), refspec(2)]);
}

#[test]
fn a_broken_mirror_gives_back_its_room_when_the_disk_is_short_for_its_clone() {
    let place = Place::new("broken-room");
    let cpython = named("python/cpython");
    let broken = place.folders.repository(&cpython).join("mirror.broken");
    fs::create_dir_all(&broken).unwrap();
    let measured = Arc::new(AtomicU64::new(0));
    let counting = Arc::clone(&measured);
    // Short until the broken mirror has gone.
    let remotes = place.clone_source().on_disk(
        Box::new(|_| Ok(1 << 20)),
        Box::new(move |_| {
            Ok(if counting.fetch_add(1, Ordering::SeqCst) == 0 {
                0
            } else {
                u64::MAX
            })
        }),
    );
    let (_versions, answers) = channel();
    let (mirrors, starts) =
        place.start_faked_from(remotes, Announce::cloning_only(Box::new(|_| {})), answers);

    mirrors.requests().clone_mirror(cpython.clone(), download());
    let clone = starts.recv_timeout(PLENTY).unwrap();
    let is_clone = clone.args.first().is_some_and(|arg| arg == "clone");
    drop(clone);
    mirrors.stop_cleanly(PLENTY);

    assert!(is_clone);
    assert!(!broken.exists());
    assert_eq!(measured.load(Ordering::SeqCst), 2);
}
