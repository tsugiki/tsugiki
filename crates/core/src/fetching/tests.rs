use super::testing::{handle, handle_with_headless, named, replied};
use super::*;
use crate::facts::CommitSha;
use crate::repository_settings::{RepositorySettings, SavedBy};
use crate::status::ItemUnread;
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

    headless.note(&pr(7), NoHead::Unnamed);
    headless.note(&pr(8), NoHead::Unnamed);
    headless.note(&pr(7), NoHead::Unnamed);
    fetching.fetch_pull_request(cpython(), 7);

    assert_eq!(fetching.why_no_head(&cpython(), 7), None);
    assert_eq!(fetching.why_no_head(&cpython(), 8), Some(NoHead::Unnamed));
}

#[test]
fn a_pr_holds_why_its_last_conversation_read_left_it_with_no_head() {
    let (fetching, _incoming, _heard, headless) = handle_with_headless();
    let seven = Item {
        repository: cpython(),
        number: 7,
    };

    headless.note(&seven, NoHead::Unnamed);
    headless.note(&seven, NoHead::Unread(ItemUnread::Other));

    assert_eq!(
        fetching.why_no_head(&cpython(), 7),
        Some(NoHead::Unread(ItemUnread::Other))
    );
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
    let saved = |by| SettingsSave::new(RepositorySettings::new(cpython()), &[], by);
    fetching.settings_changed(saved(SavedBy::HandEdit));
    fetching.settings_changed(saved(SavedBy::Window));
    let unseen = AnsweredMark::Unseen {
        number: 9,
        archived: None,
    };
    fetching.mark_answered(cpython(), unseen, replied().0);
    fetching.sync(cpython());
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
            "unseen #9, answered",
            "other",
            "stop"
        ]
    );
}

#[test]
fn a_mark_reaches_the_session_before_each_mark_sent_after_it_whichever_waits_for_an_answer() {
    let (fetching, incoming, _heard) = handle();
    let key = ItemKey::Issue { number: 5 };
    let author = || Login::new("some-author");

    // Nothing waits for an answer here, so Archive then Pin can't end pinned and Archived, nor Mark Unseen
    // then Mark Seen end unseen. Each waited-for mark is followed by one nothing waits for.
    for mark in each_answered_mark() {
        fetching.mark_answered(cpython(), mark, replied().0);
        fetching.mark_seen(cpython(), seen_5());
    }
    fetching.unmute(cpython(), key);
    fetching.keep_archived(cpython(), key, seen_5());
    fetching.unmute_author(cpython(), author());
    fetching.mark_answered(cpython(), AnsweredMark::Pin { number: 5 }, replied().0);

    let heard: Vec<String> = incoming.try_iter().map(named).collect();
    assert_eq!(
        heard,
        [
            "unseen #5, answered",
            "seen [5]",
            "archived #5, answered",
            "seen [5]",
            "unarchived #5, answered",
            "seen [5]",
            "muted #5, answered",
            "seen [5]",
            "muted author some-author, answered",
            "seen [5]",
            "pinned #5, answered",
            "seen [5]",
            "unpinned #5, answered",
            "seen [5]",
            "unmuted #5",
            "archived #5",
            "unmuted author some-author",
            "pinned #5, answered",
        ]
    );
}

/// Mark Seen on Issue 5, with nothing new on it.
fn seen_5() -> SeenMark {
    SeenMark {
        numbers: vec![5],
        up_to: None,
        findings: vec![],
    }
}

/// Each mark the window waits on, on Issue 5 or its author, in the order `AnsweredMark` names them.
fn each_answered_mark() -> [AnsweredMark; 7] {
    let key = ItemKey::Issue { number: 5 };
    [
        AnsweredMark::Unseen {
            number: 5,
            archived: Some(key),
        },
        AnsweredMark::Archive {
            key,
            mark: seen_5(),
        },
        AnsweredMark::Unarchive { key },
        AnsweredMark::Mute {
            key,
            mark: seen_5(),
        },
        AnsweredMark::MuteAuthor {
            author: Login::new("some-author"),
        },
        AnsweredMark::Pin { number: 5 },
        AnsweredMark::Unpin { number: 5 },
    ]
}

#[test]
fn a_check_with_no_session_says_tsugiki_isnt_connected() {
    let (fetching, incoming, _heard) = handle();
    drop(incoming);

    let problem = fetching.check(cpython()).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(problem.message, NOT_CONNECTED);
}

#[test]
fn each_mark_the_window_waits_for_with_no_session_says_tsugiki_isnt_connected() {
    let (fetching, incoming, _heard) = handle();
    drop(incoming);

    for mark in each_answered_mark() {
        let (reply, answer) = replied();
        fetching.mark_answered(cpython(), mark, reply);
        assert_eq!(
            answer.try_recv().unwrap().unwrap_err().message,
            NOT_CONNECTED
        );
    }
}
