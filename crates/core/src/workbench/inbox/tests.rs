use time::Duration;

use super::*;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::workbench::TreeEntry;
use crate::workbench::testing::{ViewWorld, fixing, issue, mine, stranger};

/// A moment well after the epoch, so two days before it is too.
fn now() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::days(100)
}

fn ago(hours: i64) -> OffsetDateTime {
    now() - Duration::hours(hours)
}

fn issue_at(number: u32, updated_at: OffsetDateTime) -> IssueFacts {
    IssueFacts {
        updated_at,
        ..issue(number)
    }
}

/// Someone else's PR that names Issue `issue` in its title, or no Issue for `None`.
fn pr_at(number: u32, issue: Option<u32>, updated_at: OffsetDateTime) -> PullRequestFacts {
    let title = issue.map_or_else(
        || format!("PR {number}"),
        |issue| format!("gh-{issue}: Fix"),
    );
    PullRequestFacts {
        number,
        title,
        updated_at,
        ..stranger()
    }
}

/// Each item as its kind and number, with the PRs it shows after it.
fn outline(inbox: &Inbox) -> Vec<(&'static str, u32, Vec<u32>)> {
    (inbox.items.iter())
        .map(|listed| match &listed.item {
            WorkbenchItem::Issue(issue) => {
                let rows = issue.pull_requests.iter().flat_map(TreeEntry::rows);
                ("issue", issue.number, rows.map(|row| row.number).collect())
            }
            WorkbenchItem::StandalonePr(tree) => {
                let beneath = tree.backports.iter().map(|below| below.row.number);
                ("standalone", tree.row.number, beneath.collect())
            }
            WorkbenchItem::StandaloneBackport(backport) => {
                ("standalone backport", backport.row.number, vec![])
            }
        })
        .collect()
}

fn read(world: &ViewWorld, page: InboxPage) -> Inbox {
    Inbox::read(&world.reader, &world.settings(), page, now()).unwrap()
}

#[test]
fn open_issues_come_with_their_open_prs_under_them_and_standalone_prs_on_their_own_latest_first() {
    let world = ViewWorld::new("inbox-rows");
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..pr_at(13, Some(1), ago(1))
    };
    world.has(
        vec![
            pr_at(10, Some(1), ago(5)),
            pr_at(11, Some(1), ago(3)),
            merged,
            pr_at(20, None, ago(4)),
            pr_at(21, Some(99), ago(6)),
        ],
        vec![issue_at(1, ago(10)), issue_at(2, ago(2))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 2, vec![]),
            ("issue", 1, vec![11, 10]),
            ("standalone", 20, vec![]),
            ("standalone", 21, vec![]),
        ],
        "an Issue takes the latest update of its tree, and a PR naming an Issue the cache lacks stands alone"
    );
    assert_eq!(latest.older, None);
}

#[test]
fn an_item_that_closed_leaves_the_rows() {
    let world = ViewWorld::new("inbox-closed");
    let closed_issue = IssueFacts {
        state: Some(IssueState::Closed),
        ..issue_at(1, ago(1))
    };
    let closed_pr = PullRequestFacts {
        state: PrState::Closed,
        ..pr_at(30, None, ago(1))
    };
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..pr_at(31, Some(2), ago(1))
    };
    world.has(
        vec![pr_at(10, Some(1), ago(2)), closed_pr, merged],
        vec![closed_issue, issue_at(2, ago(3))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [("issue", 2, vec![])],
        "a closed Issue leaves with its open PR, and a merged PR leaves its open Issue"
    );
}

#[test]
fn the_first_page_holds_the_last_two_days_and_the_next_the_older_batch() {
    let world = ViewWorld::new("inbox-pages");
    world.has(
        vec![pr_at(10, Some(3), ago(30)), pr_at(20, None, ago(50))],
        vec![
            issue_at(1, ago(1)),
            issue_at(2, ago(47)),
            issue_at(3, ago(100)),
            issue_at(4, ago(49)),
            issue_at(5, ago(24 * 30)),
        ],
    );

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 1, vec![]),
            ("issue", 3, vec![10]),
            ("issue", 2, vec![])
        ],
        "an old Issue whose PR moved in the last two days comes with it"
    );
    assert_eq!(after, InboxCursor::at(ago(48)));
    assert_eq!(
        outline(&older),
        [
            ("issue", 4, vec![]),
            ("standalone", 20, vec![]),
            ("issue", 5, vec![])
        ]
    );
    assert_eq!(older.older, None);
}

#[test]
fn an_older_page_holds_a_batch_and_says_where_the_next_starts() {
    let world = ViewWorld::new("inbox-batches");
    let count = OLDER_BATCH + 10;
    let hours = |number: u32| ago(100 + i64::from(number));
    let issues = (1..=count).map(|number| issue_at(number, hours(number)));
    // Every fifth Issue has a PR, which places the Issue from the groups and not from the store's batch.
    let prs = (1..=count)
        .filter(|number| number.is_multiple_of(5))
        .map(|number| pr_at(1000 + number, Some(number), hours(number)));
    world.has(prs.collect(), issues.collect());

    let latest = read(&world, InboxPage::Latest);
    let first = read(
        &world,
        InboxPage::Older {
            after: latest.older.unwrap(),
        },
    );
    let second = read(
        &world,
        InboxPage::Older {
            after: first.older.unwrap(),
        },
    );
    world.finish();

    let numbers = |inbox: &Inbox| -> Vec<u32> {
        outline(inbox)
            .into_iter()
            .map(|(_, number, _)| number)
            .collect()
    };
    assert_eq!(outline(&latest), []);
    let batch = usize::try_from(OLDER_BATCH).unwrap();
    let all: Vec<u32> = (1..=count).collect();
    assert_eq!(numbers(&first), all[..batch]);
    assert_eq!(numbers(&second), all[batch..]);
    assert_eq!(second.older, None);
}

#[test]
fn an_older_page_with_nothing_of_its_own_gives_way_to_the_next() {
    let world = ViewWorld::new("inbox-skip");
    let count = OLDER_BATCH;
    // The store's batch holds only Issues with a PR newer than the cursor, and an older one lies past it.
    let issues = (1..=count).map(|number| issue_at(number, ago(200 + i64::from(number))));
    let prs = (1..=count).map(|number| pr_at(1000 + number, Some(number), ago(1)));
    let older_issue = issue_at(500, ago(24 * 60));
    world.has(prs.collect(), issues.chain([older_issue]).collect());

    let latest = read(&world, InboxPage::Latest);
    let older = read(
        &world,
        InboxPage::Older {
            after: latest.older.unwrap(),
        },
    );
    world.finish();

    assert_eq!(latest.items.len(), usize::try_from(count).unwrap());
    assert_eq!(outline(&older), [("issue", 500, vec![])]);
    assert_eq!(older.older, None);
}

#[test]
fn rows_carry_what_the_workbench_rows_carry() {
    let world = ViewWorld::new("inbox-attention");
    let requested = PullRequestFacts {
        review_requests: vec![crate::workbench::testing::me()],
        ..pr_at(10, Some(1), ago(1))
    };
    world.has(vec![requested, mine(20)], vec![issue_at(1, ago(2))]);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    let why = (latest.items.first())
        .and_then(|listed| listed.attention.as_ref())
        .map(|attention| attention.why.clone());
    assert_eq!(why, Some(vec![crate::work::Why::ReviewRequested]));
}

#[test]
fn nothing_is_listed_before_anyone_signs_in() {
    let folder = crate::testing::temp_path("inbox-no-viewer");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("tsugiki.db");
    let store = crate::store::Store::open(&path).unwrap();
    let name = crate::workbench::testing::cpython().name;
    store.save_issues(&name, vec![issue_at(1, ago(1))]).unwrap();
    store
        .save_pull_requests(&name, vec![fixing(10, 1)])
        .unwrap();
    let (to_open, reader) = crate::store::reader();
    let open = to_open.open(&path).unwrap();

    let read = Inbox::read(
        &reader,
        &crate::workbench::testing::cpython(),
        InboxPage::Latest,
        now(),
    );

    open.close();
    store.close();
    let read = read.unwrap();
    assert_eq!(outline(&read), []);
}

#[test]
fn the_window_sends_a_page_and_gets_the_next_one_s_start_as_text() {
    let asked: InboxPage = serde_json::from_value(serde_json::json!({
        "kind": "older",
        "after": { "updated_at": "1970-04-11T00:00:00Z", "number": 7 },
    }))
    .unwrap();
    let latest: InboxPage =
        serde_json::from_value(serde_json::json!({ "kind": "latest" })).unwrap();
    let inbox = Inbox {
        items: vec![],
        older: Some(InboxCursor::at(now())),
    };

    let sent = serde_json::to_value(&inbox).unwrap();

    let after = InboxCursor {
        updated_at: now(),
        number: 7,
    };
    assert_eq!(asked, InboxPage::Older { after });
    assert_eq!(latest, InboxPage::Latest);
    assert_eq!(
        sent["older"],
        serde_json::json!({ "updated_at": "1970-04-11T00:00:00Z", "number": 0 })
    );
}
