use std::io;
use std::sync::mpsc::Receiver;

use serde_json::json;
use time::OffsetDateTime;
use tsugiki_token::testing::{gh_alone, gh_prints};

use super::*;
use crate::github::testing::{
    RATE_LIMIT, Scripted, answer, github, not_found, replied, signed_in_as, token,
};
use crate::github::{Grant, Reply, Unreachable};
use crate::status::Status;
use crate::status::testing::changes_board;
use crate::store::testing::{OpenStore, refuse_read_only_marks};
use crate::testing::{cpython, gcmon};
use crate::token::{GhOutput, Token, read_token};

/// Long enough that no test waits it out.
const AN_HOUR: Duration = Duration::from_secs(3600);

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

/// GitHub's answer that the Repository asked about has `visibility`, to a token that writes to public
/// repositories alone and doesn't expire, as each answer names the token's scopes again.
fn visibility(visibility: Visibility) -> Result<Reply, Unreachable> {
    let private = visibility == Visibility::Private;
    let answer = answer(
        json!({ "repository": { "isPrivate": private } }),
        RATE_LIMIT,
    );
    answer.map(|reply| Reply {
        grant: Grant {
            scopes: Some(WriteScope::Public),
            expires: None,
        },
        ..reply
    })
}

/// A client that sends `token`, whose first answer is `first` and then `rest`, once it has asked who is
/// signed in, and what it asks from then on.
fn answered_to(
    token: Token,
    first: Result<Reply, Unreachable>,
    rest: Vec<Result<Reply, Unreachable>>,
) -> (GitHub, Receiver<(String, serde_json::Value)>) {
    let (transport, asked) = Scripted::new([vec![first], rest].concat());
    let github = GitHub::new(Box::new(transport), token);
    assert_eq!(github.viewer().unwrap().login.as_str(), "octocat");
    let viewer = asked.recv().unwrap();
    assert_eq!(viewer.0, "Viewer");
    (github, asked)
}

/// [`answered_to`] gh's OAuth token.
fn after_answer(
    first: Result<Reply, Unreachable>,
    rest: Vec<Result<Reply, Unreachable>>,
) -> (GitHub, Receiver<(String, serde_json::Value)>) {
    answered_to(token(), first, rest)
}

/// A gh that prints a token.
type Gh = fn() -> io::Result<GhOutput>;

fn fine_grained_gh() -> io::Result<GhOutput> {
    gh_prints("github_pat_example")
}

fn app_gh() -> io::Result<GhOutput> {
    gh_prints("ghu_example")
}

fn both() -> Vec<RepositoryName> {
    vec![cpython(), gcmon()]
}

/// What a turn of the GitHub thread hands the judge.
fn parts<'a>(board: &'a mut StatusBoard, db: &'a OpenStore, github: &'a GitHub) -> ThreadParts<'a> {
    ThreadParts {
        board,
        store: &db.store,
        github,
    }
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
    let mut judge = WriteJudge::new(AN_HOUR);

    judge.follow(parts(&mut board, &db, &github), &both());

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
    let mut judge = WriteJudge::new(AN_HOUR);

    judge.follow(parts(&mut board, &db, &github), &both());

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
    let (mut board, changes) = changes_board();
    let (github, _asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::new(AN_HOUR);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);
    let public_repo = db.read_only_marks();

    let (github, asked) = after_answer(granted(Some(WriteScope::Every), None), vec![]);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(public_repo, (true, false));
    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(asked.try_iter().count(), 0);
    assert_eq!(heard(&changes), [cpython(), cpython()]);
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
    let mut judge = WriteJudge::new(AN_HOUR);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);
    let first: Vec<_> = asked.try_iter().collect();
    assert_eq!(heard(&changes), [cpython()]);

    judge.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(first.len(), 1);
    assert_eq!(asked.try_iter().count(), 0);
    assert_eq!(heard(&changes), []);
    db.close();
}

#[test]
fn a_visibility_github_couldnt_answer_waits_before_it_is_asked_again() {
    let db = OpenStore::new("judge-retry");
    let (mut board, _changes) = changes_board();
    let (github, asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![Err(Unreachable::Offline)],
    );
    let mut judge = WriteJudge::new(AN_HOUR);

    judge.follow(parts(&mut board, &db, &github), &both());
    let first: Vec<_> = asked.try_iter().collect();
    judge.follow(parts(&mut board, &db, &github), &both());

    assert_eq!(
        first.len(),
        1,
        "gcmon waits, as GitHub couldn't answer CPython"
    );
    assert_eq!(asked.try_iter().count(), 0, "the next turn waits too");
    assert_eq!(db.read_only_marks(), (false, false));
    db.close();
}

#[test]
fn a_visibility_github_couldnt_answer_is_asked_again_once_the_wait_is_over() {
    let db = OpenStore::new("judge-retried");
    let (mut board, _changes) = changes_board();
    let (github, _asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![Err(Unreachable::Offline), visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::new(Duration::ZERO);

    judge.follow(parts(&mut board, &db, &github), &[cpython()]);
    let unjudged = db.read_only_marks();
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(unjudged, (false, false));
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}

#[test]
fn a_server_error_on_a_visibility_question_keeps_the_mark_and_asks_again_later() {
    let db = OpenStore::new("judge-server-error");
    let (mut board, changes) = changes_board();
    let (github, _asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![
            visibility(Visibility::Private),
            replied(502, "Bad Gateway"),
            visibility(Visibility::Private),
        ],
    );
    let mut judge = WriteJudge::new(Duration::ZERO);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);
    assert_eq!(heard(&changes), [cpython()]);

    // The next run asks again, and GitHub stumbles once.
    let mut next_run = WriteJudge::new(Duration::ZERO);
    next_run.follow(parts(&mut board, &db, &github), &[cpython()]);
    let kept = db.read_only_marks();
    next_run.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(kept, (true, false));
    assert_eq!(db.read_only_marks(), (true, false));
    assert_eq!(heard(&changes), []);
    db.close();
}

#[test]
fn a_repository_the_settings_drop_loses_its_mark() {
    let db = OpenStore::new("judge-dropped");
    let (mut board, changes) = changes_board();
    let (github, _asked) = after_answer(granted(Some(WriteScope::None), None), vec![]);
    let mut judge = WriteJudge::new(AN_HOUR);
    judge.follow(parts(&mut board, &db, &github), &both());
    assert_eq!(heard(&changes), both());

    judge.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(db.read_only_marks(), (true, false));
    assert_eq!(heard(&changes), [gcmon()]);
    db.close();
}

#[test]
fn a_mark_the_store_refuses_goes_to_the_log_and_leaves_the_status_alone() {
    let db = OpenStore::new("judge-refused");
    refuse_read_only_marks(&db.store);
    let (said, statuses) = std::sync::mpsc::channel();
    let mut board = StatusBoard::new(Box::new(move |status: &Status| {
        said.send(status.clone()).unwrap();
    }));
    let (github, _asked) = after_answer(granted(Some(WriteScope::None), None), vec![]);
    let mut judge = WriteJudge::new(AN_HOUR);

    judge.follow(parts(&mut board, &db, &github), &both());

    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(statuses.try_iter().count(), 0);
    db.close();
}

#[test]
fn offline_no_visibility_is_asked_until_github_is_back() {
    let db = OpenStore::new("judge-offline");
    let (mut board, _changes) = changes_board();
    let (github, asked) = after_answer(
        granted(Some(WriteScope::Public), None),
        vec![visibility(Visibility::Private)],
    );
    let mut judge = WriteJudge::new(AN_HOUR);

    board.set(Status::Offline);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);
    let offline = asked.try_iter().count();
    board.set(Status::Connecting);
    judge.follow(parts(&mut board, &db, &github), &[cpython()]);

    assert_eq!(offline, 0);
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}

#[test]
fn another_token_clears_the_marks_before_github_can_say_anything_else() {
    let db = OpenStore::new("judge-another-token");
    let (mut board, changes) = changes_board();
    let (github, _asked) = after_answer(granted(Some(WriteScope::None), None), vec![]);
    WriteJudge::new(AN_HOUR).follow(parts(&mut board, &db, &github), &both());
    assert_eq!(heard(&changes), both());

    // The next run's token writes to public repositories alone, and GitHub goes away after its first
    // answer.
    let renewed = OffsetDateTime::from_unix_timestamp(2_100_000_000).ok();
    let (github, asked) = after_answer(granted(Some(WriteScope::Public), renewed), vec![]);
    board.set(Status::Offline);
    WriteJudge::new(AN_HOUR).follow(parts(&mut board, &db, &github), &both());

    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(heard(&changes), both());
    assert_eq!(asked.try_iter().count(), 0);
    db.close();
}

#[test]
fn a_token_of_another_kind_clears_the_marks_before_github_first_answers() {
    let db = OpenStore::new("judge-another-kind");
    let classic = TokenSeen {
        kind: TokenKind::Classic,
        expires: None,
    };
    let gh = TokenSeen {
        kind: TokenKind::OAuth,
        expires: None,
    };
    assert!(db.store.mark_read_only(classic, &cpython()).unwrap());
    assert!(db.store.mark_read_only(gh, &gcmon()).unwrap());
    let (mut board, changes) = changes_board();
    let (transport, _asked) = Scripted::new(vec![]);
    let github = github(transport);

    WriteJudge::new(AN_HOUR).follow(parts(&mut board, &db, &github), &both());

    assert_eq!(db.read_only_marks(), (false, true));
    assert_eq!(heard(&changes), [cpython()]);
    db.close();
}

#[test]
fn a_token_whose_answers_name_no_scopes_keeps_its_marks() {
    let db = OpenStore::new("judge-silent");
    let (mut board, changes) = changes_board();
    let expires = OffsetDateTime::from_unix_timestamp(2_000_000_000).ok();
    let (github, _asked) = after_answer(granted(Some(WriteScope::None), expires), vec![]);
    let mut judge = WriteJudge::new(AN_HOUR);
    judge.follow(parts(&mut board, &db, &github), &both());
    assert_eq!(heard(&changes), both());

    let (github, _asked) = after_answer(granted(None, expires), vec![]);
    judge.follow(parts(&mut board, &db, &github), &both());

    assert_eq!(db.read_only_marks(), (true, true));
    assert_eq!(heard(&changes), []);
    db.close();
}

#[test]
fn the_scopes_github_names_for_a_fine_grained_or_an_app_token_mark_nothing() {
    let tokens: [(&str, Gh); 2] = [
        ("judge-fine-grained", fine_grained_gh),
        ("judge-app", app_gh),
    ];
    for (name, gh) in tokens {
        let db = OpenStore::new(name);
        let (mut board, changes) = changes_board();
        let token = read_token(gh_alone(gh)).unwrap();
        // An empty `X-OAuth-Scopes` header names no scope that writes.
        let (github, asked) = answered_to(token, granted(Some(WriteScope::None), None), vec![]);

        WriteJudge::new(AN_HOUR).follow(parts(&mut board, &db, &github), &both());

        assert_eq!(db.read_only_marks(), (false, false), "{name}");
        assert_eq!(heard(&changes), [], "{name}");
        assert_eq!(asked.try_iter().count(), 0, "{name}");
        db.close();
    }
}

#[test]
fn before_github_first_answers_nothing_is_judged() {
    let db = OpenStore::new("judge-unanswered");
    let (mut board, changes) = changes_board();
    let (transport, _asked) = Scripted::new(vec![]);
    let github = github(transport);

    WriteJudge::new(AN_HOUR).follow(parts(&mut board, &db, &github), &both());

    assert_eq!(db.read_only_marks(), (false, false));
    assert_eq!(heard(&changes), []);
    db.close();
}
