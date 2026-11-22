use serde_json::Value;

use super::*;
use crate::facts::activity::ActivityFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::settings::ListOrder;
use crate::testing::{cpython, cpython_settings};
use crate::workbench::testing::{
    ViewWorld, at, first_issue, fixing, grouped_whole, issue, mine, outline, reasons_for, stranger,
    workbench,
};

#[test]
fn an_issue_with_no_pr_of_the_users_isnt_listed() {
    let other = PullRequestFacts {
        title: "gh-1: Fix it".to_owned(),
        ..stranger()
    };
    let found = workbench(vec![other], vec![issue(1), issue(2)]);
    assert_eq!(outline(&found), []);
}

#[test]
fn a_pinned_issue_no_filter_reaches_is_on_the_workbench_marked_pinned_until_unpinned() {
    let world = ViewWorld::new("workbench-pinned");
    let closed = crate::facts::issues::IssueFacts {
        state: Some(crate::facts::issues::IssueState::Closed),
        ..issue(1)
    };
    // GitHub numbers Issues and PRs apart, and the store refuses a pin on a PR's number.
    let theirs = PullRequestFacts {
        number: 10,
        ..stranger()
    };
    world.has(vec![theirs], vec![closed, issue(2)]);
    let store = &world.db.store;
    store.pin(&cpython(), 1).unwrap();
    let read = || {
        let (view, order) = (&WorkbenchView::Whole, ListOrder::Attention);
        Workbench::read(&world.db.reader, &world.settings(), view, order).unwrap()
    };

    let pinned = read();
    crate::store::testing::unpin(store, &cpython(), 1);
    let unpinned = read();

    world.finish();
    // A pin holds the Issue as a Filter would, closed or not, with the stranger's PR in its tree.
    assert_eq!(outline(&pinned), [("issue", 1, vec![10])]);
    assert!(first_issue(&pinned).pinned);
    let sent = serde_json::to_value(&pinned).unwrap();
    assert_eq!(sent["items"][0]["pinned"], true);
    assert_eq!(outline(&unpinned), []);
}

#[test]
fn the_pinned_view_lists_the_pins_in_the_workbench_order_and_counts_those_that_need_attention() {
    let world = ViewWorld::new("workbench-pinned-view");
    let updated = |number, seconds| crate::facts::issues::IssueFacts {
        updated_at: at(seconds),
        ..issue(number)
    };
    let issues = vec![
        updated(1, 5),
        updated(2, 1),
        updated(3, 3),
        updated(4, 4),
        updated(5, 9),
    ];
    world.has(vec![], issues);
    let store = &world.db.store;
    for number in 1..=4 {
        store.pin(&cpython(), number).unwrap();
    }
    let now = time::OffsetDateTime::now_utc();
    for number in [2, 5] {
        let flagging = crate::store::Flagging::on(number);
        store.mark_unseen(&cpython(), flagging, now).unwrap();
    }
    let archiving = crate::store::Archiving {
        key: ItemKey::Issue { number: 3 },
        numbers: vec![3],
        up_to: None,
        put_away: crate::facts::activity::PutAway::Now,
        findings: vec![],
    };
    store.archive(&cpython(), archiving, now).unwrap();
    world.mute_issue(4, vec![4]);
    let read = |view: &WorkbenchView| {
        Workbench::read(
            &world.db.reader,
            &world.settings(),
            view,
            ListOrder::Attention,
        )
        .unwrap()
    };

    let pinned = read(&WorkbenchView::Pinned);
    let whole = read(&WorkbenchView::Whole);
    crate::store::testing::unpin(store, &cpython(), 2);
    let unpinned = read(&WorkbenchView::Pinned);

    world.finish();
    // What Needs attention comes first, then the rest by last update. The Archived pin stays in its place,
    // marked, and the Muted one stays hidden. Issue 5 has no pin, so it Needs attention on the Workbench
    // alone.
    assert_eq!(
        outline(&pinned),
        [
            ("issue", 2, vec![]),
            ("issue", 1, vec![]),
            ("issue", 3, vec![])
        ]
    );
    let hidden: Vec<Option<Hidden>> = pinned.items.iter().map(|listed| listed.hidden).collect();
    assert_eq!(hidden, [None, None, Some(Hidden::Archived)]);
    assert_eq!((pinned.pinned, whole.pinned, whole.count), (1, 1, 2));
    let sent = serde_json::to_value(&pinned).unwrap();
    assert_eq!(sent["pinned"], 1);
    assert_eq!(
        outline(&unpinned),
        [("issue", 1, vec![]), ("issue", 3, vec![])]
    );
    assert_eq!(unpinned.pinned, 0);
}

#[test]
fn a_pr_of_someone_elses_with_no_issue_isnt_listed() {
    let found = workbench(vec![stranger()], vec![]);
    assert_eq!(outline(&found), []);
}

#[test]
fn items_come_latest_first_and_a_tie_goes_to_the_larger_number() {
    let latest = PullRequestFacts {
        updated_at: at(9),
        ..mine(30)
    };
    let tied = PullRequestFacts {
        updated_at: at(5),
        ..mine(20)
    };
    let under = PullRequestFacts {
        updated_at: at(5),
        ..fixing(10, 1)
    };
    let found = workbench(vec![tied, latest, under], vec![issue(1)]);
    assert_eq!(
        outline(&found),
        [
            ("standalone", 30, vec![]),
            ("standalone", 20, vec![]),
            ("issue", 1, vec![10]),
        ]
    );
    assert_eq!(first_issue(&found).number, 1);
}

#[test]
fn the_window_gets_the_kind_of_each_item_and_its_times_as_text() {
    let found = workbench(vec![mine(3), fixing(4, 1)], vec![issue(1)]);
    let sent = serde_json::to_value(&found).unwrap();
    assert_eq!(sent["items"][0]["kind"], "standalone_pr");
    assert_eq!(sent["items"][0]["number"], 3);
    assert_eq!(sent["items"][0]["updated_at"], "1970-01-01T00:00:01Z");
    assert_eq!(sent["items"][0]["attention"], Value::Null);
    assert_eq!(sent["items"][1]["kind"], "issue");
    assert_eq!(sent["items"][1]["updated_at"], "1970-01-01T00:00:01Z");
    assert_eq!(sent["items"][1]["pull_requests"][0]["kind"], "pr");
    assert_eq!(sent["items"][1]["pull_requests"][0]["number"], 4);
}

#[test]
fn a_merged_pr_of_the_users_keeps_no_issue_on_the_workbench() {
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..fixing(10, 1)
    };
    assert_eq!(reasons_for(&merged), []);
    assert_eq!(outline(&workbench(vec![merged], vec![issue(1)])), []);
}

#[test]
fn an_items_marks_are_its_own_and_each_of_its_rows_backports_included() {
    let world = ViewWorld::new("workbench-marks");
    world.has_crossed_links();
    let (items, _) = grouped_whole(&world, &world.settings());
    let shapes = |items: &[WorkbenchItem]| -> Vec<&'static str> {
        let shape = |item: &WorkbenchItem| match item {
            WorkbenchItem::Issue(issue) if issue.pull_requests.iter().any(is_backport) => "issue",
            WorkbenchItem::Issue(_) => "",
            WorkbenchItem::StandalonePr(_) => "pr",
            WorkbenchItem::StandaloneBackport(_) => "backport",
        };
        let mut shapes: Vec<_> = items.iter().map(shape).filter(|s| !s.is_empty()).collect();
        shapes.sort_unstable();
        shapes.dedup();
        shapes
    };
    assert_eq!(
        shapes(&items),
        ["backport", "issue", "pr"],
        "the fixture's shapes"
    );

    for mut item in items {
        let own = item.seen().mark.numbers.clone();
        let rows = item.rows().into_iter().map(|row| vec![row.number]);
        let expected: Vec<Vec<u32>> = match &item {
            WorkbenchItem::Issue(_) => std::iter::once(own).chain(rows).collect(),
            WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => rows.collect(),
        };
        let marks: Vec<Vec<u32>> = (item.marks_mut().into_iter())
            .map(|mark| mark.numbers.clone())
            .collect();
        assert_eq!(marks, expected);
    }
    world.finish();
}

/// Whether `entry` is a Backport whose Origin PR Tsugiki can't show.
fn is_backport(entry: &TreeEntry) -> bool {
    matches!(entry, TreeEntry::Backport(_))
}

#[test]
fn nothing_is_the_users_before_anyone_signs_in() {
    let found = Workbench::from_facts(
        WorkFacts {
            activity: ActivityFacts::default(),
            reviews: ReviewFacts::default(),
            viewer: None,
            pull_requests: vec![mine(1)],
            issues: vec![issue(1)],
            filters: BTreeMap::new(),
        },
        &cpython_settings(),
        &WorkbenchView::Whole,
    );
    assert_eq!(found.items, []);
}
