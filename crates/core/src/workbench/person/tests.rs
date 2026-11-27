use time::OffsetDateTime;

use super::*;
use crate::facts::activity::{ItemKey, PutAway};
use crate::facts::inbox_search::{InboxSearch, RowKind, SearchHit};
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::PullRequestFacts;
use crate::settings::ListOrder;
use crate::store::{Archiving, Muting};
use crate::testing::{cpython, me};
use crate::workbench::WorkbenchView;
use crate::workbench::search::Search;
use crate::workbench::testing::{ViewWorld, at, issue, outline, stranger};

/// A PR numbered `number`, by `author`, naming Issue `fixes` in its title.
fn by(
    author: &str,
    number: u32,
    fixes: Option<u32>,
    updated_at: OffsetDateTime,
) -> PullRequestFacts {
    let title = fixes.map_or_else(
        || format!("PR {number}"),
        |fixes| format!("gh-{fixes}: Fix it"),
    );
    PullRequestFacts {
        number,
        title,
        author: Some(Login::new(author)),
        updated_at,
        ..stranger()
    }
}

/// The person Filter for what `login` authored.
fn read(world: &ViewWorld, login: &str) -> Workbench {
    let view = WorkbenchView::Person {
        login: Login::new(login),
    };
    Workbench::read(
        &world.db.reader,
        &world.settings(),
        &view,
        ListOrder::Attention,
    )
    .unwrap()
}

/// GitHub's answer listing `hits`, of `total` results, and no more to read.
fn answer(hits: Vec<(u32, RowKind)>, total: u32) -> InboxSearch {
    InboxSearch {
        hits: (hits.into_iter().enumerate())
            .map(|(position, (number, kind))| SearchHit {
                number,
                kind,
                updated_at: at(i64::try_from(position).unwrap()),
            })
            .collect(),
        total,
        cut_off: false,
    }
}

#[test]
fn a_person_s_filter_lists_what_the_cache_finds_at_once() {
    let world = ViewWorld::new("person-cached");
    world.has(
        vec![
            by("some-author", 10, Some(1), at(40)),
            by("some-author", 20, None, at(30)),
            by("other-author", 30, None, at(20)),
        ],
        vec![issue(1)],
    );

    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![10]), ("standalone", 20, vec![])],
        "the person's PRs narrow the Issue's tree, and their own PRs stand alone"
    );
    assert_eq!(
        found.filter_query.as_ref().map(FilterQuery::as_str),
        Some("is:open author:some-author"),
        "the Filter's query is the one a typed search runs"
    );
}

#[test]
fn the_person_s_issues_show_alone_when_no_pr_of_theirs_fixes_one() {
    let world = ViewWorld::new("person-issues");
    let theirs = IssueFacts {
        author: Some(Login::new("some-author")),
        ..issue(2)
    };
    world.has(
        vec![by("other-author", 30, Some(2), at(10))],
        vec![issue(1), theirs],
    );

    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 2, vec![])],
        "an Issue the person authored stays alone, as another author's PR that fixes it doesn't match"
    );
}

#[test]
fn a_person_s_filter_leaves_out_their_closed_issues() {
    let world = ViewWorld::new("person-closed");
    let theirs = |number, state| IssueFacts {
        author: Some(Login::new("some-author")),
        state,
        ..issue(number)
    };
    world.has(
        vec![],
        vec![
            theirs(1, Some(IssueState::Open)),
            theirs(2, Some(IssueState::Closed)),
        ],
    );

    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![])],
        "the person Filter lists their open Issues and leaves out the closed ones"
    );
}

#[test]
fn a_bot_is_a_person_too() {
    let world = ViewWorld::new("person-bot");
    world.has(vec![by("dependabot[bot]", 20, None, at(1))], vec![]);

    let found = read(&world, "dependabot[bot]");
    world.finish();

    assert_eq!(outline(&found), [("standalone", 20, vec![])]);
    assert_eq!(
        found.filter_query.as_ref().map(FilterQuery::as_str),
        Some("is:open author:dependabot[bot]")
    );
}

#[test]
fn github_s_answer_decides_the_order_and_keeps_a_placeholder_for_what_the_cache_lacks() {
    let world = ViewWorld::new("person-answer");
    let theirs = IssueFacts {
        author: Some(Login::new("some-author")),
        ..issue(2)
    };
    world.has(
        vec![
            by("some-author", 10, Some(1), at(40)),
            by("some-author", 20, None, at(30)),
            by("other-author", 30, None, at(20)),
        ],
        vec![issue(1), theirs],
    );
    world
        .db
        .store
        .save_inbox_search(
            &cpython(),
            "is:open author:some-author",
            answer(
                vec![
                    (20, RowKind::PullRequest),
                    (2, RowKind::Issue),
                    (40, RowKind::PullRequest),
                ],
                3,
            ),
        )
        .unwrap();

    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        outline(&found),
        [
            ("standalone", 20, vec![]),
            ("issue", 2, vec![]),
            ("placeholder", 40, vec![]),
        ],
        "GitHub's order stands, the PR it left out drops, and a result the cache lacks keeps its place"
    );
}

#[test]
fn the_filter_shows_the_sidebar_the_repository_s_own_figures() {
    let world = ViewWorld::new("person-figures");
    world.has(
        vec![
            by("some-author", 10, Some(1), at(40)),
            by(me().as_str(), 20, None, at(30)),
        ],
        vec![issue(1)],
    );

    let whole = Workbench::read(
        &world.db.reader,
        &world.settings(),
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        (
            found.count,
            found.review_requests,
            found.stale_reviews,
            found.pinned
        ),
        (
            whole.count,
            whole.review_requests,
            whole.stale_reviews,
            whole.pinned
        ),
        "a person Filter changes nothing on the Workbench"
    );
    assert_eq!(found.filters, whole.filters);
}

#[test]
fn an_archived_item_stays_in_place_and_a_muted_one_folds_at_the_end() {
    let world = ViewWorld::new("person-hidden");
    world.has(
        vec![
            by("some-author", 10, Some(1), at(40)),
            by("some-author", 20, None, at(30)),
        ],
        vec![issue(1)],
    );
    let store = &world.db.store;
    store
        .archive(
            &cpython(),
            Archiving {
                key: ItemKey::Issue { number: 1 },
                numbers: vec![1, 10],
                up_to: None,
                put_away: PutAway::Now,
                findings: vec![],
            },
            OffsetDateTime::UNIX_EPOCH + time::Duration::days(100),
        )
        .unwrap();
    store
        .mute(
            &cpython(),
            Muting {
                key: ItemKey::PullRequest { number: 20 },
                numbers: vec![20],
                up_to: None,
                findings: vec![],
            },
            OffsetDateTime::UNIX_EPOCH + time::Duration::days(100),
        )
        .unwrap();

    let found = read(&world, "some-author");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![10])],
        "an Archived item stays in its place, as a Filter's view lists it"
    );
    assert_eq!(found.items[0].hidden, Some(Hidden::Archived));
    assert_eq!(
        crate::workbench::testing::outline_of(&found.muted),
        [("standalone", 20, vec![])],
        "a Muted item goes in the fold at the end"
    );
}

#[test]
fn the_search_reads_the_query_the_filter_names() {
    // The query a person Filter runs is one the cache narrows by itself, so the cached matches show
    // before GitHub answers at all.
    let search = Search::parse("is:open author:some-author");
    assert!(!search.needs_github());
    assert!(!search.is_empty());
}
