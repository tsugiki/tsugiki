use std::sync::mpsc::Receiver;

use serde_json::json;
use time::OffsetDateTime;

use super::*;
use crate::github::testing::{RATE_LIMIT, Scripted, answer, github, not_found, signed_in_as};
use crate::github::{Grant, Reply, Unreachable};
use crate::status::testing::changes_board;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, gcmon};

/// GitHub's answer that the user is signed in, to a token whose scopes reach `scopes`, which expires at
/// `expires`.
fn granted(
    scopes: Option<WriteScope>,
    expires: Option<OffsetDateTime>,
) -> Result<Reply, Unreachable> {
    signed_in_as("octocat").map(|reply| Reply {
        grant: Grant { scopes, expires },
        ..reply
    })
}

/// GitHub's answer that the Repository asked about has `visibility`.
fn visibility(visibility: Visibility) -> Result<Reply, Unreachable> {
    let private = visibility == Visibility::Private;
    answer(
        json!({ "repository": { "isPrivate": private } }),
        RATE_LIMIT,
    )
}

/// A client whose first answer is `first` and then `rest`, once it has asked who is signed in, and what it
/// asks from then on.
fn after_answer(
    first: Result<Reply, Unreachable>,
    rest: Vec<Result<Reply, Unreachable>>,
) -> (GitHub, Receiver<(String, serde_json::Value)>) {
    let (transport, asked) = Scripted::new([vec![first], rest].concat());
    let github = github(transport);
    assert_eq!(github.viewer().unwrap().login.as_str(), "octocat");
    let viewer = asked.recv().unwrap();
    assert_eq!(viewer.0, "Viewer");
    (github, asked)
}

fn both() -> Vec<RepositoryName> {
    vec![cpython(), gcmon()]
}

/// The Repositories the window heard of since the test last looked.
fn heard(changes: &Receiver<Change>) -> Vec<RepositoryName> {
    changes
        .try_iter()
        .map(|change| {
            assert_eq!(
                change.items,
                Vec::<u32>::new(),
                "a change of state names no item"
            );
            change.repository
        })
        .collect()
}

#[test]
fn a_token_without_repo_or_public_repo_makes_every_repository_read_only_with_nothing_sent() {
    let db = OpenStore::new("judge-no-scope");
    let (mut board, changes) = changes_board();
    let (github, asked) = after_answer(granted(Some(WriteScope::None), None), vec![]);
    let mut judge = WriteJudge::default();

    tell(&mut board, judge.follow(&db.store, &github, both()));

    assert_eq!(db.read_only_marks(), (true, true));
    assert_eq!(heard(&changes), both());
    assert_eq!(
        asked.try_iter().count(),
        0,
        "nothing asked, nothing written"
    );
    db.close();
}

#[test]
fn public_repo_alone_makes_the_private_repositories_read_only() {
    let db = OpenStore::new("judge-public-repo");
    let (mut board, changes) = changes_board();
    let gone = not_found(json!({ "repository": null }), json!(["repository"]));
    let (github, asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![visibility(Visibility::Public), gone],
    );
    let mut judge = WriteJudge::default();

    tell(&mut board, judge.follow(&db.store, &github, both()));

    // GitHub tells a token that can't see a private Repository that it doesn't exist.
    assert_eq!(db.read_only_marks(), (false, true));
    assert_eq!(heard(&changes), [gcmon()]);
    let operations: Vec<String> = asked.try_iter().map(|(operation, _)| operation).collect();
    assert_eq!(operations, ["RepositoryVisibility", "RepositoryVisibility"]);
    db.close();
}

#[test]
fn a_private_repository_github_names_is_read_only_too_and_repo_writes_to_it() {
    let db = OpenStore::new("judge-private");
    let (mut board, _changes) = changes_board();
    let (github, _asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::default();
    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );
    let public_repo = db.read_only_marks();

    let (github, asked) = after_answer(granted(Some(WriteScope::Every), None), vec![]);
    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );

    assert_eq!(public_repo, (true, false));
    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(asked.try_iter().count(), 0);
    db.close();
}

#[test]
fn an_answer_that_changes_nothing_asks_nothing_and_tells_nothing() {
    let db = OpenStore::new("judge-unchanged");
    let (mut board, changes) = changes_board();
    let (github, asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::default();
    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );
    let first: Vec<_> = asked.try_iter().collect();
    assert_eq!(heard(&changes), [cpython()]);

    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );

    assert_eq!(first.len(), 1);
    assert_eq!(asked.try_iter().count(), 0);
    assert_eq!(heard(&changes), []);
    db.close();
}

#[test]
fn a_visibility_github_cant_answer_now_waits_for_a_later_turn() {
    let db = OpenStore::new("judge-offline-visibility");
    let (mut board, _changes) = changes_board();
    let (github, _asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![Err(Unreachable::Offline), visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::default();

    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );
    let unjudged = db.read_only_marks();
    tell(
        &mut board,
        judge.follow(&db.store, &github, vec![cpython()]),
    );

    assert_eq!(unjudged, (false, false));
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}

#[test]
fn a_token_whose_answers_name_no_scopes_keeps_its_marks_and_another_token_clears_them() {
    let db = OpenStore::new("judge-silent");
    let (mut board, changes) = changes_board();
    let expires = OffsetDateTime::from_unix_timestamp(2_000_000_000).ok();
    let (github, _asked) = after_answer(granted(Some(WriteScope::None), expires), vec![]);
    let mut judge = WriteJudge::default();
    tell(&mut board, judge.follow(&db.store, &github, both()));
    assert_eq!(heard(&changes), both());

    let (github, _asked) = after_answer(granted(None, expires), vec![]);
    tell(&mut board, judge.follow(&db.store, &github, both()));
    let kept = db.read_only_marks();
    let renewed = OffsetDateTime::from_unix_timestamp(2_100_000_000).ok();
    let (github, _asked) = after_answer(granted(None, renewed), vec![]);
    tell(&mut board, judge.follow(&db.store, &github, both()));

    assert_eq!(kept, (true, true));
    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(heard(&changes), both());
    db.close();
}

#[test]
fn before_github_first_answers_nothing_is_judged() {
    let db = OpenStore::new("judge-unanswered");
    let (mut board, changes) = changes_board();
    let (transport, _asked) = Scripted::new(vec![]);
    let github = github(transport);
    let mut judge = WriteJudge::default();

    tell(&mut board, judge.follow(&db.store, &github, both()));

    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(heard(&changes), []);
    db.close();
}
