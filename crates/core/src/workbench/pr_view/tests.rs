use time::OffsetDateTime;

use super::testing::reviewed;
use super::*;
use crate::facts::pull_requests::PullRequestFacts;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, cpython_settings};
use crate::workbench::issue_view::IssueView;
use crate::workbench::testing::{
    ViewWorld, backport, bots_backport, fixing, grouped_whole, issue, someone, stranger, under_each,
};

fn view(world: &ViewWorld, number: u32) -> Option<PrView> {
    PrView::read(&world.db.reader, &world.settings(), number).unwrap()
}

#[test]
fn a_pr_tsugiki_doesnt_hold_has_no_view() {
    let world = ViewWorld::new("pr-view-none");
    world.has(vec![], vec![issue(1)]);

    assert_eq!(view(&world, 1), None, "an Issue is no PR");
    assert_eq!(view(&world, 5), None);
    world.finish();
}

#[test]
fn neither_view_shows_anything_before_anyone_signs_in() {
    let db = OpenStore::new("views-no-viewer");
    db.store
        .save_pull_requests(&cpython(), vec![fixing(5, 1)])
        .unwrap();
    db.store.save_issues(&cpython(), vec![issue(1)]).unwrap();

    let pr = PrView::read(&db.reader, &cpython_settings(), 5).unwrap();
    let issue = IssueView::read(&db.reader, &cpython_settings(), 1).unwrap();

    db.close();
    assert_eq!((pr, issue), (None, None));
}

#[test]
fn a_pr_in_an_issues_tree_shows_its_row_before_tsugiki_has_read_its_conversation() {
    let world = ViewWorld::new("pr-view-before-reading");
    world.has(vec![fixing(5, 1)], vec![issue(1)]);

    let view = view(&world, 5).unwrap();

    assert_eq!(view.row.number, 5);
    assert_eq!(view.row.title, "gh-1: Fix it");
    assert_eq!(view.content, None);
    world.finish();
}

#[test]
fn a_pr_says_whether_the_item_that_holds_it_is_muted() {
    let world = ViewWorld::new("pr-view-muted");
    world.has(vec![fixing(5, 1)], vec![issue(1)]);
    assert_eq!(view(&world, 5).unwrap().hidden, None);

    world.mute_issue(1, vec![1, 5]);

    assert_eq!(view(&world, 5).unwrap().hidden, Some(Hidden::Muted));
    world.finish();
}

#[test]
fn a_pr_names_each_issue_that_holds_it_and_whether_it_is_pinned() {
    let world = ViewWorld::new("pr-view-pinned");
    let both = PullRequestFacts {
        closing_issues: vec![3],
        ..fixing(5, 1)
    };
    let alone = PullRequestFacts {
        number: 6,
        title: "PR 6".to_owned(),
        ..stranger()
    };
    world.has(vec![both, alone], vec![issue(1), issue(3)]);
    world.db.store.pin(&cpython(), 3).unwrap();

    let holding = view(&world, 5).unwrap().issues;
    let standalone = view(&world, 6).unwrap().issues;

    world.finish();
    let held = |number, pinned| HoldingIssue { number, pinned };
    assert_eq!(holding, [held(1, false), held(3, true)]);
    assert_eq!(standalone, [], "only an Issue takes a pin");
}

#[test]
fn a_bot_backport_names_the_origin_pr_whose_review_state_it_shares() {
    let world = ViewWorld::new("pr-view-shared-review");
    let manual = backport(3, "[3.14] gh-1: Fix it (GH-1)", vec![Some(someone())]);
    world.has(
        vec![
            stranger(),
            bots_backport(2, "[3.14] gh-1: Fix it (GH-1)"),
            manual,
        ],
        vec![issue(1)],
    );

    assert_eq!(view(&world, 2).unwrap().shares_review_of, Some(1));
    assert_eq!(
        view(&world, 3).unwrap().shares_review_of,
        None,
        "a Manual backport"
    );
    assert_eq!(
        view(&world, 1).unwrap().shares_review_of,
        None,
        "the Origin PR"
    );
    world.finish();
}

#[test]
fn a_pr_shows_its_reviews_oldest_first_with_their_threads_as_last_read() {
    let world = ViewWorld::new("pr-view-after-reading");
    let standalone = PullRequestFacts {
        number: 9,
        ..stranger()
    };
    world.has(vec![standalone], vec![]);
    world
        .db
        .store
        .save_pull_request_content(
            &world.settings().name,
            9,
            (reviewed(), OffsetDateTime::UNIX_EPOCH),
        )
        .unwrap();

    let view = view(&world, 9).unwrap();

    assert_eq!(view.row.number, 9);
    assert_eq!(view.content, Some(reviewed()));
    world.finish();
}

#[test]
fn the_window_gets_githubs_number_for_each_comment_and_review() {
    let world = ViewWorld::new("pr-view-numbers");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    world
        .db
        .store
        .save_pull_request_content(
            &world.settings().name,
            9,
            (reviewed(), OffsetDateTime::UNIX_EPOCH),
        )
        .unwrap();

    let sent = serde_json::to_value(view(&world, 9).unwrap()).unwrap();

    let content = &sent["content"];
    assert_eq!(
        content["comments"][0]["id"],
        serde_json::json!(6_065_108_504_u64)
    );
    assert_eq!(
        content["reviews"][0]["id"],
        serde_json::json!(5_481_796_336_u64)
    );
    assert_eq!(content["reviews"][1]["id"], serde_json::Value::Null);
    world.finish();
}

#[test]
fn each_read_of_a_pr_replaces_its_reviews() {
    let world = ViewWorld::new("pr-view-read-again");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    let name = world.settings().name;
    world
        .db
        .store
        .save_pull_request_content(&name, 9, (reviewed(), OffsetDateTime::UNIX_EPOCH))
        .unwrap();
    let mut again = reviewed();
    // The read again lacks the first Review.
    again.reviews = again.reviews.split_off(1);
    again.earlier_reviews = 0;

    world
        .db
        .store
        .save_pull_request_content(&name, 9, (again.clone(), OffsetDateTime::UNIX_EPOCH))
        .unwrap();

    assert_eq!(view(&world, 9).unwrap().content, Some(again));
    world.finish();
}

#[test]
fn a_pr_read_before_tsugiki_read_reviews_has_none_until_the_next_read() {
    let world = ViewWorld::new("pr-view-no-reviews-yet");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    let conversation = reviewed().conversation;
    world
        .db
        .store
        .save_issue_content(&world.settings().name, 9, conversation.clone())
        .unwrap();

    let content = view(&world, 9).unwrap().content.unwrap();

    assert_eq!(content.conversation, conversation);
    assert_eq!((content.reviews, content.earlier_reviews), (vec![], 0));
    world.finish();
}

#[test]
fn a_pr_shows_the_row_the_grouping_of_every_pr_gives_it_under_each_conventions() {
    let world = ViewWorld::new("pr-view-whole");
    world.has_crossed_links();

    for settings in under_each(&world.settings()) {
        let (items, causes) = grouped_whole(&world, &settings);
        let facts = world.db.reader.work_facts(&settings.name).unwrap();
        let shared = SharedReviews::of(&facts.pull_requests, settings.conventions);
        for number in facts.pull_requests.iter().map(|pr| pr.number) {
            let conventions = settings.conventions;
            let shown = PrView::read(&world.db.reader, &settings, number)
                .unwrap()
                .unwrap();
            let holding = (items.iter())
                .find(|item| item.rows().iter().any(|row| row.number == number))
                .unwrap();

            let row = holding.rows().into_iter().find(|row| row.number == number);
            assert_eq!(Some(&shown.row), row, "PR {number} under {conventions:?}");
            let hidden = Hidden::of(&causes, holding.key(), &holding.numbers());
            assert_eq!(shown.hidden, hidden, "PR {number} under {conventions:?}");
            assert_eq!(shown.shares_review_of, shared.origin_of(number));
        }
    }
    world.finish();
}
