use std::sync::Mutex;
use std::time::Instant;

use super::testing::Fixture;
use super::*;
use crate::mirror::{HeardRequests, LookupCheckout};

fn query(text: &str) -> Query {
    Query {
        text: text.to_owned(),
        case: Case::Ignored,
        words: Words::Within,
        syntax: Syntax::Literal,
    }
}

fn repository() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// Waits up to 10 seconds for `done` to hold.
fn wait_until(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A text search whose questions reach the test, and how many times it announced a change.
fn started() -> (TextSearch, HeardRequests, Arc<Mutex<u32>>) {
    let (requests, heard) = HeardRequests::new();
    let announced = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&announced);
    let search = TextSearch::start(requests, Box::new(move || *counter.lock().unwrap() += 1));
    (search, heard, announced)
}

/// The next question of where a Lookup checkout is, once it comes.
fn asked(heard: &HeardRequests) -> Sender<Option<LookupCheckout>> {
    let mut found = None;
    wait_until(|| {
        found = heard.checkouts().pop();
        found.is_some()
    });
    let (repository, reply) = found.unwrap();
    assert_eq!(repository, self::repository());
    reply
}

/// The results of query `id` once it has ended, each file as its path and its lines' numbers.
fn ended(search: &TextSearch, id: SearchId) -> (Vec<(String, Vec<u32>)>, End, u32) {
    let mut last = None;
    wait_until(|| {
        last = match search.state() {
            SearchState::Results(results) => {
                Some(results).filter(|results| results.id == id && results.end != End::Running)
            }
            SearchState::Idle
            | SearchState::Waiting { .. }
            | SearchState::NoCheckout { .. }
            | SearchState::Failed { .. } => None,
        };
        last.is_some()
    });
    let results = last.unwrap();
    let files = (results.files.iter())
        .map(|file| {
            let lines = file.lines.iter().map(|line| line.line).collect();
            (file.path.clone(), lines)
        })
        .collect();
    (files, results.end, results.matches)
}

fn pr(name: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("b.py", b"spam\neggs\nspam spam\n"),
            ("a.py", b"eggs\n"),
            ("c.py", b"nothing\n"),
        ],
        &[("a.py", Some(b"spam\n".as_slice()))],
    )
}

#[test]
fn a_query_finds_the_files_by_path_with_their_matches_and_says_it_is_done() {
    let fixture = pr("search-owner");
    let (search, heard, announced) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    asked(&heard).send(Some(fixture.checkout.clone())).unwrap();
    let (files, end, matches) = ended(&search, id);
    assert_eq!(
        files,
        [
            ("a.py".to_owned(), vec![1]),
            ("b.py".to_owned(), vec![1, 3])
        ]
    );
    assert_eq!((end, matches), (End::Done, 4));
    assert!(*announced.lock().unwrap() >= 2);
}

#[test]
fn a_query_says_that_it_waits_for_the_checkout_to_move() {
    let fixture = pr("search-owner-waits");
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    let reply = asked(&heard);
    wait_until(|| matches!(search.state(), SearchState::Waiting { id: waiting } if waiting == id));
    reply.send(Some(fixture.checkout.clone())).unwrap();
    let (files, end, _) = ended(&search, id);
    assert_eq!((files.len(), end), (2, End::Done));
}

#[test]
fn a_second_query_cancels_the_first() {
    let fixture = pr("search-owner-cancels");
    let (search, heard, _) = started();
    let first = search.search(repository(), &query("spam")).unwrap();
    let first_reply = asked(&heard);
    let second = search.search(repository(), &query("eggs")).unwrap();
    assert_ne!(first, second);
    let second_reply = asked(&heard);
    second_reply.send(Some(fixture.checkout.clone())).unwrap();
    let (files, _, _) = ended(&search, second);
    assert_eq!(files, [("b.py".to_owned(), vec![2])]);
    // The first goes on to search once its checkout comes, and nothing it finds reaches the window.
    first_reply.send(Some(fixture.checkout.clone())).unwrap();
    thread::sleep(Duration::from_millis(200));
    let SearchState::Results(results) = search.state() else {
        panic!("the second query's results are gone");
    };
    assert_eq!(results.id, second);
    assert_eq!(results.files.len(), 1);
}

#[test]
fn a_query_with_no_checkout_says_so() {
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    asked(&heard).send(None).unwrap();
    wait_until(|| matches!(search.state(), SearchState::NoCheckout { id: none } if none == id));
}

#[test]
fn a_query_whose_files_git_cannot_list_fails() {
    let mut fixture = pr("search-owner-fails");
    fixture.checkout.at.head = crate::github::CommitSha::parse(&"0".repeat(40)).unwrap();
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    asked(&heard).send(Some(fixture.checkout.clone())).unwrap();
    wait_until(|| matches!(search.state(), SearchState::Failed { id: failed } if failed == id));
}

#[test]
fn a_bad_regex_returns_its_error_and_leaves_the_query_running_alone() {
    let fixture = pr("search-owner-bad");
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    let bad = Query {
        syntax: Syntax::Regex,
        ..query("(")
    };
    let refused = search.search(repository(), &bad).err().unwrap();
    assert!(
        refused.message.contains("unclosed group"),
        "{}",
        refused.message
    );
    asked(&heard).send(Some(fixture.checkout.clone())).unwrap();
    let (files, end, _) = ended(&search, id);
    assert_eq!((files.len(), end), (2, End::Done));
    assert!(heard.checkouts().is_empty());
}

#[test]
fn with_no_network_a_query_searches_a_commit_the_mirror_holds() {
    let fixture = pr("search-owner-offline");
    // Nothing answers at this address, so a fetch would fail.
    let _ = tsugiki_test_git::git(
        &fixture.mirror,
        &[
            "remote",
            "add",
            "origin",
            "https://127.0.0.1:9/python/cpython.git",
        ],
    );
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    asked(&heard).send(Some(fixture.checkout.clone())).unwrap();
    let (files, end, _) = ended(&search, id);
    assert_eq!((files.len(), end), (2, End::Done));
}

#[test]
fn a_search_past_the_most_matches_stops_and_says_so() {
    let text = format!("{}\n", "spam ".repeat(40)).repeat(150);
    let fixture = Fixture::new("search-owner-most", &[("many.txt", text.as_bytes())], &[]);
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    asked(&heard).send(Some(fixture.checkout.clone())).unwrap();
    let (_, end, matches) = ended(&search, id);
    assert_eq!((end, matches), (End::Stopped, MOST_MATCHES));
}

#[test]
fn a_worker_that_stops_without_an_answer_leaves_no_checkout() {
    let (search, heard, _) = started();
    let id = search.search(repository(), &query("spam")).unwrap();
    drop(asked(&heard));
    wait_until(|| matches!(search.state(), SearchState::NoCheckout { id: none } if none == id));
}

#[test]
fn a_search_at_once_gives_the_files_by_path() {
    let fixture = pr("search-once");
    let (files, end) = search_checkout(&fixture.checkout, &query("spam")).unwrap();
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!((paths, end), (vec!["a.py", "b.py"], End::Done));
}

#[test]
fn a_search_at_once_refuses_a_bad_query_and_a_commit_git_lacks() {
    let mut fixture = pr("search-once-refused");
    let refused = search_checkout(&fixture.checkout, &query("")).unwrap_err();
    assert_eq!(refused.message, "There's nothing to search for.");
    fixture.checkout.at.base = crate::github::CommitSha::parse(&"0".repeat(40)).unwrap();
    let failed = search_checkout(&fixture.checkout, &query("spam")).unwrap_err();
    assert_eq!(failed.message, "Tsugiki couldn't read the PR's files.");
}
