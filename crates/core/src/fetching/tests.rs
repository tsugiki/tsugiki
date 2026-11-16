use std::sync::mpsc::{self, Receiver};

use super::*;
use crate::facts::CommitSha;
use crate::mirror::HeardRequests;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn head(number: u32) -> PrHead {
    PrHead {
        number,
        commit: CommitSha::parse(&format!("{number:040x}")).unwrap(),
        target_branch: "main".to_owned(),
    }
}

/// A handle, what reaches the session through it, and what reaches the Mirror worker.
fn handle() -> (Fetching, Receiver<SessionMessage>, HeardRequests) {
    let (messages, incoming) = mpsc::channel();
    let (mirrors, heard) = HeardRequests::new();
    (Fetching::new(messages, mirrors), incoming, heard)
}

#[test]
fn opening_a_pr_asks_the_mirror_for_what_its_head_lacks() {
    let (fetching, _incoming, heard) = handle();

    fetching.pull_request_opened(cpython(), 7, HeadInMirror::Held);
    fetching.pull_request_opened(cpython(), 8, HeadInMirror::Unknown);
    fetching.pull_request_opened(cpython(), 9, HeadInMirror::Missing(head(9)));

    assert_eq!(
        heard.asked(),
        ["python/cpython opened #7", "python/cpython awaits #8"]
    );
    assert_eq!(heard.firsts(), [(cpython(), head(9))]);
}

#[test]
fn a_want_hands_the_mirror_each_of_its_parts_and_then_the_heads_saved() {
    let (fetching, _incoming, heard) = handle();
    let want = MirrorWant {
        heads: vec![head(7)],
        reviewed: Some(vec![ReviewedCommit {
            number: 8,
            commit: head(8).commit,
        }]),
        patches: vec![],
    };

    fetching.want(&cpython(), want);
    fetching.heads_saved(&cpython(), vec![head(7)]);

    assert_eq!(
        heard.asked(),
        [
            "python/cpython wants heads #7 at 0000000 on main",
            "python/cpython wants reviewed #8 at 0000000",
            "python/cpython wants patch-ids of none",
            "python/cpython saved heads #7 at 0000000 on main",
        ]
    );
}

#[test]
fn reviewed_commits_the_store_couldnt_read_leave_the_last_want_of_them() {
    let (fetching, _incoming, heard) = handle();
    let want = MirrorWant {
        heads: vec![],
        reviewed: None,
        patches: vec![],
    };

    fetching.want(&cpython(), want);

    assert_eq!(
        heard.asked(),
        [
            "python/cpython wants heads none",
            "python/cpython wants patch-ids of none",
        ]
    );
}

#[test]
fn the_users_demands_of_github_reach_the_session_in_the_order_asked() {
    let (fetching, incoming, _heard) = handle();

    fetching.fetch_issue(cpython(), 7);
    fetching.fetch_pull_request(cpython(), 8);
    fetching.refresh();
    fetching.mark_unseen(cpython(), 9);
    fetching.stop_session();

    let heard: Vec<String> = incoming
        .try_iter()
        .map(|message| match message {
            SessionMessage::Fetch(Fetched::Issue(item)) => format!("Issue #{}", item.number),
            SessionMessage::Fetch(Fetched::PullRequest(item)) => format!("PR #{}", item.number),
            SessionMessage::Refresh => "refresh".to_owned(),
            SessionMessage::Mark(Marked::Unseen { item, .. }) => format!("unseen #{}", item.number),
            SessionMessage::Stop => "stop".to_owned(),
            SessionMessage::Check { .. }
            | SessionMessage::Sync { .. }
            | SessionMessage::Mark(Marked::Seen { .. })
            | SessionMessage::Patched(..) => "other".to_owned(),
        })
        .collect();
    assert_eq!(heard, ["Issue #7", "PR #8", "refresh", "unseen #9", "stop"]);
}

#[test]
fn a_check_with_no_session_says_tsugiki_isnt_connected() {
    let (fetching, incoming, _heard) = handle();
    drop(incoming);

    let problem = fetching.check(cpython()).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(problem.message, NOT_CONNECTED);
}
