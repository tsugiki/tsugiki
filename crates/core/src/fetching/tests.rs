use super::testing::{handle, handle_with_headless, named};
use super::*;
use crate::facts::CommitSha;
use crate::testing::cpython;

fn head(number: u32) -> PrHead {
    PrHead {
        number,
        commit: CommitSha::parse(&format!("{number:040x}")).unwrap(),
        target_branch: "main".to_owned(),
    }
}

#[test]
fn a_pr_whose_conversation_named_no_head_twice_has_a_head_again_after_one_demand() {
    let (fetching, _incoming, _heard, headless) = handle_with_headless();
    let pr = |number| Item {
        repository: cpython(),
        number,
    };

    headless.named_none(&pr(7));
    headless.named_none(&pr(8));
    headless.named_none(&pr(7));
    fetching.fetch_pull_request(cpython(), 7);

    assert!(!fetching.named_no_head(&cpython(), 7));
    assert!(fetching.named_no_head(&cpython(), 8));
}

#[test]
fn opening_a_pr_asks_the_mirror_for_what_its_head_lacks() {
    let (fetching, _incoming, heard) = handle();

    fetching.pull_request_opened(cpython(), 7, HeadInMirror::Held);
    fetching.pull_request_opened(cpython(), 8, HeadInMirror::Unknown);
    fetching.pull_request_opened(cpython(), 9, HeadInMirror::Missing(head(9)));

    assert_eq!(heard.firsts(), [(cpython(), head(9))]);
    assert_eq!(
        heard.asked(),
        ["python/cpython opened #7", "python/cpython awaits #8"]
    );
}

#[test]
fn the_diff_on_show_moves_the_lookup_checkout_to_its_ends() {
    let (fetching, _incoming, heard) = handle();
    let at = LookupAt {
        base: head(1).commit,
        head: head(2).commit,
    };

    fetching.pull_request_opened(cpython(), 2, HeadInMirror::Missing(head(2)));
    fetching.look_up(cpython(), at.clone());

    assert_eq!(heard.look_ups(), [(cpython(), at)]);
    assert_eq!(heard.firsts(), [(cpython(), head(2))]);
}

#[test]
fn a_mirror_whose_clone_waits_for_nothing_doesnt_ask_where_its_objects_come_from() {
    let (fetching, _incoming, _heard) = handle();

    assert!(!fetching.mirror_asks(&cpython()));
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
    fetching.settings_changed(RepositorySettings::new(cpython()), SavedBy::HandEdit);
    fetching.settings_changed(RepositorySettings::new(cpython()), SavedBy::Window);
    fetching.mark_unseen(cpython(), 9, None);
    fetching.sync(RepositorySettings::new(cpython()));
    fetching.stop_session();

    let heard: Vec<String> = incoming.try_iter().map(named).collect();
    assert_eq!(
        heard,
        [
            "Issue #7",
            "PR #8",
            "refresh",
            "settings of python/cpython",
            "switch",
            "unseen #9",
            "other",
            "stop"
        ]
    );
}

#[test]
fn a_check_with_no_session_says_tsugiki_isnt_connected() {
    let (fetching, incoming, _heard) = handle();
    drop(incoming);

    let problem = fetching.check(cpython()).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(problem.message, NOT_CONNECTED);
}
