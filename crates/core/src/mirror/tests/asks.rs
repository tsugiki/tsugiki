//! The head of a PR the user opened, kept apart from the heads the rules want (#899). Each test names the
//! case of #899 it covers. A fetch's refspecs don't name a commit, so each test checks the commit through
//! what the app hears: a fetch that asked for the wrong one finds its head behind.

use std::thread;

use super::*;

/// How long a head that is behind waits here before it is fetched once more: long enough that the test
/// starts what it means to before then, on a busy machine too.
const PATIENCE: Duration = Duration::from_millis(500);

const MAIN: &str = "+refs/heads/main*:refs/heads/main*";

/// The git the worker starts next, once it has handled what the test sent.
fn next_start(mirrors: &Mirrors, starts: &Receiver<FakeStart>) -> FakeStart {
    mirrors.wait_idle();
    starts.try_recv().expect("a git started")
}

/// Waits until each head behind by now is due to be fetched once more.
fn wait_out_patience() {
    thread::sleep(PATIENCE * 2);
}

/// Stops the worker, and says that nothing started after the last git the test took.
fn stop_with_nothing_more(mirrors: Mirrors, starts: &Receiver<FakeStart>) {
    mirrors.stop_cleanly(PLENTY);
    let more = starts.recv();
    assert!(more.is_err(), "{:?}", more.map(|start| start.input));
}

fn heard_in(repository: &str, numbers: &[u32]) -> (String, Vec<u32>) {
    (repository.to_owned(), numbers.to_vec())
}

#[test]
fn a_hand_over_of_the_commit_the_rules_fetch_brought_leaves_the_commit_the_user_opened() {
    // Case 1.
    let place = Place::new("ask-stale-hand-over");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(5)]);
    let wanted = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_first(cpython.clone(), newer(5));
    // Offline, nothing starts once the fetch has brought `head(5)`, until after the hand-over below.
    requests.follow(&Account::Offline);
    wanted.succeed();
    requests.fetch_heads(cpython, vec![head(5)]);
    requests.follow(&Account::Online {
        login: Login::new("octocat"),
    });
    let asked = next_start(&mirrors, &starts);
    assert_eq!(asked.refspecs(), [refspec(5)]);
    assert_eq!(asked.branches(), [MAIN]);
    asked.succeed_with(vec![newer(5)]);
    stop_with_nothing_more(mirrors, &starts);

    // Of PR 5, the app hears only of the user's head.
    let heard: Vec<_> = arrivals.iter().collect();
    assert_eq!(heard, [heard_in("python/cpython", &[5])]);
    assert!(failures.try_recv().is_err());
}

#[test]
fn a_hand_over_during_the_fetch_of_its_own_that_fails_leaves_the_commit_the_user_opened_first() {
    // Case 2.
    let place = Place::new("ask-failed-own");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PLENTY);
    let requests = mirrors.requests();

    requests.fetch_first(cpython.clone(), newer(5));
    let asked = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython, vec![head(5), head(6)]);
    asked.fail();
    let again = next_start(&mirrors, &starts);
    assert_eq!(again.refspecs(), [refspec(5)], "the user's head goes alone");
    assert_eq!(again.branches(), [MAIN]);
    again.succeed_with(vec![newer(5)]);
    // The rules still want PR 5 at the commit they handed over, which the mirror doesn't hold.
    let wanted = next_start(&mirrors, &starts);
    assert_eq!(wanted.refspecs(), [refspec(5), refspec(6)]);
    wanted.succeed_with(vec![newer(5), head(6)]);
    stop_with_nothing_more(mirrors, &starts);

    let cpython = "python/cpython";
    let heard: Vec<_> = arrivals.iter().collect();
    assert_eq!(heard, [heard_in(cpython, &[5]), heard_in(cpython, &[6])]);
    assert!(failures.try_recv().is_err());
}

#[test]
fn a_head_the_user_opened_that_the_rules_fetch_finds_behind_is_retried_in_a_fetch_of_its_own() {
    // Case 3.
    let place = Place::new("ask-behind-wanted");
    let (cpython, hello) = (named("python/cpython"), named("octo-org/hello"));
    place.mirrored(&cpython);
    place.mirrored(&hello);
    let (mirrors, starts, _arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![head(5)]);
    let first = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_first(cpython.clone(), head(5));
    requests.fetch_heads(cpython, vec![head(5), head(6)]);
    // GitHub hadn't moved `refs/pull/5/head` yet.
    first.succeed_with(vec![older(5)]);
    let wanted = next_start(&mirrors, &starts);
    assert_eq!(wanted.refspecs(), [refspec(6)]);
    wait_out_patience();
    requests.fetch_heads(hello, vec![head(7)]);
    wanted.succeed();
    let again = next_start(&mirrors, &starts);
    assert_eq!(
        again.refspecs(),
        [refspec(5)],
        "the user's head goes ahead of hello's"
    );
    assert_eq!(again.branches(), [MAIN]);
    again.succeed();
    let hellos = next_start(&mirrors, &starts);
    assert_eq!(
        hellos.refspecs(),
        [refspec(7)],
        "the fetch brought the rules' head 5"
    );
    hellos.succeed();
    stop_with_nothing_more(mirrors, &starts);

    assert!(failures.try_recv().is_err());
}

#[test]
fn a_head_whose_fetch_of_its_own_finds_it_behind_is_retried_in_a_fetch_of_its_own() {
    // Case 3, with the head's own fetch.
    let place = Place::new("ask-behind-own");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, _arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_first(cpython.clone(), head(5));
    let asked = starts.recv_timeout(PLENTY).unwrap();
    requests.fetch_heads(cpython.clone(), vec![head(6)]);
    asked.succeed_with(vec![older(5)]);
    let wanted = next_start(&mirrors, &starts);
    assert_eq!(wanted.refspecs(), [refspec(6)]);
    wait_out_patience();
    requests.fetch_heads(cpython, vec![head(6), head(8)]);
    wanted.succeed();
    let again = next_start(&mirrors, &starts);
    assert_eq!(again.refspecs(), [refspec(5)], "the user's head goes alone");
    assert_eq!(again.branches(), [MAIN]);
    again.succeed();
    let rest = next_start(&mirrors, &starts);
    assert_eq!(rest.refspecs(), [refspec(8)]);
    rest.succeed();
    stop_with_nothing_more(mirrors, &starts);

    assert!(failures.try_recv().is_err());
}

#[test]
fn a_hand_over_of_an_older_commit_the_mirror_holds_leaves_the_commit_the_user_opened() {
    // Case 4.
    let place = Place::new("ask-older-hand-over");
    let (cpython, hello) = (named("python/cpython"), named("octo-org/hello"));
    place.mirrored(&cpython);
    place.mirrored(&hello);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![older(5)]);
    let wanted = starts.recv_timeout(PLENTY).unwrap();
    wanted.succeed_with(vec![older(5)]);
    requests.fetch_heads(hello, vec![head(7)]);
    let hellos = next_start(&mirrors, &starts);
    requests.fetch_first(cpython.clone(), head(5));
    requests.fetch_heads(cpython, vec![older(5)]);
    hellos.succeed();
    let asked = next_start(&mirrors, &starts);
    assert_eq!(asked.refspecs(), [refspec(5)]);
    assert_eq!(asked.branches(), [MAIN]);
    asked.succeed_with(vec![head(5)]);
    stop_with_nothing_more(mirrors, &starts);

    let (cpython, hello) = ("python/cpython", "octo-org/hello");
    let heard: Vec<_> = arrivals.iter().collect();
    let expected = [
        heard_in(cpython, &[5]),
        heard_in(hello, &[7]),
        heard_in(cpython, &[5]),
    ];
    assert_eq!(heard, expected);
    let failed: Vec<_> = failures.iter().collect();
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn a_hand_over_during_the_fetch_of_its_own_of_an_older_commit_the_mirror_holds_fetches_nothing() {
    // Case 5.
    let place = Place::new("ask-hand-over-during");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![older(5)]);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older(5)]);
    requests.fetch_first(cpython.clone(), head(5));
    let asked = next_start(&mirrors, &starts);
    requests.fetch_heads(cpython, vec![older(5)]);
    asked.succeed_with(vec![head(5)]);
    stop_with_nothing_more(mirrors, &starts);

    let cpython = "python/cpython";
    let heard: Vec<_> = arrivals.iter().collect();
    assert_eq!(heard, [heard_in(cpython, &[5]), heard_in(cpython, &[5])]);
    let failed: Vec<_> = failures.iter().collect();
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn a_retry_of_the_rules_older_commit_tells_the_app_nothing_of_a_pr_the_user_opened() {
    // Case 6.
    let place = Place::new("ask-older-retry");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![older(5)]);
    starts.recv_timeout(PLENTY).unwrap().succeed_with(vec![]);
    requests.fetch_first(cpython, head(5));
    let asked = next_start(&mirrors, &starts);
    assert_eq!(asked.refspecs(), [refspec(5)]);
    // The want's retry falls due while the fetch of its own runs.
    wait_out_patience();
    asked.succeed_with(vec![head(5)]);
    let retried = next_start(&mirrors, &starts);
    assert_eq!(retried.refspecs(), [refspec(5)]);
    retried.succeed_with(vec![head(5)]);
    stop_with_nothing_more(mirrors, &starts);

    let heard: Vec<_> = arrivals.iter().collect();
    assert_eq!(heard, [heard_in("python/cpython", &[5])]);
    let failed: Vec<_> = failures.iter().collect();
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn a_hand_over_leaves_the_retry_of_a_head_the_user_opened_in_a_fetch_of_its_own() {
    // Case 7.
    let place = Place::new("ask-hand-over-retry");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, arrivals, failures) = place.start_faked_hearing_all(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_heads(cpython.clone(), vec![older(5)]);
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older(5)]);
    requests.fetch_first(cpython.clone(), head(5));
    next_start(&mirrors, &starts).succeed_with(vec![older(5)]);
    requests.fetch_heads(cpython, vec![older(5), head(6)]);
    let wanted = next_start(&mirrors, &starts);
    assert_eq!(wanted.refspecs(), [refspec(6)]);
    wait_out_patience();
    wanted.succeed();
    let again = next_start(&mirrors, &starts);
    assert_eq!(again.refspecs(), [refspec(5)]);
    assert_eq!(again.branches(), [MAIN]);
    again.succeed_with(vec![head(5)]);
    stop_with_nothing_more(mirrors, &starts);

    let cpython = "python/cpython";
    let heard: Vec<_> = arrivals.iter().collect();
    let expected = [
        heard_in(cpython, &[5]),
        heard_in(cpython, &[6]),
        heard_in(cpython, &[5]),
    ];
    assert_eq!(heard, expected);
    assert!(failures.try_recv().is_err());
}

#[test]
fn the_app_hears_a_head_the_user_opened_is_behind_once_its_own_fetches_found_it_so_twice() {
    let place = Place::new("ask-behind-heard");
    let cpython = named("python/cpython");
    place.mirrored(&cpython);
    let (mirrors, starts, failures) = place.start_faked_hearing_failures(PATIENCE);
    let requests = mirrors.requests();

    requests.fetch_first(cpython, head(5));
    starts
        .recv_timeout(PLENTY)
        .unwrap()
        .succeed_with(vec![older(5)]);
    let again = starts.recv_timeout(PLENTY).unwrap();
    assert_eq!(again.refspecs(), [refspec(5)]);
    assert_eq!(again.branches(), [MAIN]);
    again.succeed_with(vec![older(5)]);
    stop_with_nothing_more(mirrors, &starts);

    let heard: Vec<_> = failures.iter().collect();
    let behind = ("python/cpython".to_owned(), vec![5], Unfetched::Behind);
    assert_eq!(heard, [behind]);
}
