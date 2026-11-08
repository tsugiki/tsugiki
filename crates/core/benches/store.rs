//! The store's reads and the view queries built on them, on a database sized like CPython's (ADR 0017).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use time::{Duration, OffsetDateTime};
use tsugiki_core::conventions::Conventions;
use tsugiki_core::filters::Filter;
use tsugiki_core::github::activity::{Activity, ActivityKind, ItemActivity};
use tsugiki_core::github::issue_content::{IssueComment, IssueContent};
use tsugiki_core::github::issues::{IssueFacts, IssueState};
use tsugiki_core::github::pull_requests::{
    CiState, LatestReview, OffDefault, PrState, PullRequestFacts, ReviewState,
};
use tsugiki_core::github::{CommitSha, Login};
use tsugiki_core::repository::{RepositoryAccess, RepositoryName, RepositorySettings};
use tsugiki_core::store::{self, FilterResults, OpenReader, Store, StoreReader};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_view::PrView;
use tsugiki_core::workbench::{Workbench, WorkbenchView};

fn main() {
    divan::main();
}

/// The PRs a first sync of CPython held on 2026-11-05 (ADR 0015). The view queries' 50 ms budget is held to
/// this size (ADR 0017).
const FIRST_SYNC_PRS: u32 = 3_631;

/// The first sync's PRs, and five times as many, a guess at a store that has kept months of refreshes and
/// opened trees.
const PRS: [u32; 2] = [FIRST_SYNC_PRS, FIRST_SYNC_PRS * 5];

/// The Issues that same first sync held.
const ISSUES: u32 = 2_368;

/// The first Issue's number, above every PR's, so the two never share one.
const FIRST_ISSUE: u32 = 200_000;

#[divan::bench(args = PRS)]
fn work_facts(bencher: divan::Bencher<'_, '_>, prs: u32) {
    let held = Held::new("work-facts", prs);
    bencher.bench(|| held.reader.work_facts(&held.repository));
    held.close();
}

#[divan::bench]
fn workbench(bencher: divan::Bencher<'_, '_>) {
    read_workbench(bencher, "workbench", |_| WorkbenchView::Whole);
}

#[divan::bench]
fn workbench_filter(bencher: divan::Bencher<'_, '_>) {
    read_workbench(bencher, "workbench-filter", |settings| {
        WorkbenchView::Filter {
            query: open_prs(settings).query.clone(),
        }
    });
}

#[divan::bench]
fn workbench_review_requests(bencher: divan::Bencher<'_, '_>) {
    read_workbench(bencher, "workbench-review-requests", |_| {
        WorkbenchView::ReviewRequests
    });
}

/// Reads the view `view_of` picks, as the Sidebar opens it, in a database of its own named `name`.
#[expect(
    clippy::expect_used,
    reason = "a benchmark that timed an empty view would measure nothing CPython's user waits for"
)]
fn read_workbench(
    bencher: divan::Bencher<'_, '_>,
    name: &str,
    view_of: fn(&RepositorySettings) -> WorkbenchView,
) {
    let held = Held::with_views(name);
    let view = view_of(&held.settings);
    let shown = Workbench::read(&held.reader, &held.settings, &view).expect("the view");
    assert!(
        !shown.items.is_empty() || !shown.quiet.is_empty(),
        "the view lists items"
    );
    bencher.bench(|| Workbench::read(&held.reader, &held.settings, &view));
    held.close();
}

/// The Issue view of an Issue that two PRs name, with its description and comments.
#[divan::bench]
#[expect(
    clippy::expect_used,
    reason = "a benchmark that timed an Issue the store doesn't hold would measure an early return"
)]
fn issue_view(bencher: divan::Bencher<'_, '_>) {
    let held = Held::with_views("issue-view");
    let shown = IssueView::read(&held.reader, &held.settings, VIEWED_ISSUE)
        .expect("the view")
        .expect("an Issue the store holds");
    assert!(shown.content.is_some() && !shown.issue.pull_requests.is_empty());
    bencher.bench(|| IssueView::read(&held.reader, &held.settings, VIEWED_ISSUE));
    held.close();
}

/// The Issue the Issue view benchmark opens. PRs name Issues by their number modulo `ISSUES`, so PRs 7 and
/// 2,375 name this one.
const VIEWED_ISSUE: u32 = FIRST_ISSUE + 7;

/// The PR the PR view benchmark opens, in the viewed Issue's tree.
const VIEWED_PR: u32 = 7;

/// The PR view of a PR in an Issue's tree, with its description and comments.
#[divan::bench]
#[expect(
    clippy::expect_used,
    reason = "a benchmark that timed a PR the store doesn't hold would measure an early return"
)]
fn pr_view(bencher: divan::Bencher<'_, '_>) {
    let held = Held::with_views("pr-view");
    let shown = PrView::read(&held.reader, &held.settings, VIEWED_PR)
        .expect("the view")
        .expect("a PR the store holds");
    assert!(shown.content.is_some());
    bencher.bench(|| PrView::read(&held.reader, &held.settings, VIEWED_PR));
    held.close();
}

/// The settings of `repository`, with the starter Filters of a Repository the user maintains.
fn settings(repository: &RepositoryName) -> RepositorySettings {
    RepositorySettings {
        name: repository.clone(),
        conventions: Conventions::Cpython,
        filters: Filter::starters(RepositoryAccess::Maintained),
    }
}

#[expect(
    clippy::expect_used,
    reason = "the starter Filters of a maintained Repository include Open PRs"
)]
fn open_prs(settings: &RepositorySettings) -> &Filter {
    settings
        .filters
        .iter()
        .find(|filter| filter.query.as_str().contains("is:pr"))
        .expect("a Filter of open PRs")
}

/// A database of CPython's size, open for writing and for reading.
struct Held {
    folder: PathBuf,
    store: Store,
    open: OpenReader,
    reader: StoreReader,
    repository: RepositoryName,
    settings: RepositorySettings,
}

impl Held {
    #[expect(
        clippy::expect_used,
        reason = "a benchmark whose database can't be built has nothing to measure"
    )]
    fn new(name: &str, prs: u32) -> Self {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.temp")
            .join(format!("bench-{name}-{prs}-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a folder for the database");
        let path = folder.join("tsugiki.db");
        let store = Store::open(&path).expect("a new database");
        let repository = RepositoryName::parse("python/cpython").expect("a Repository's name");
        store.remember_login(&viewer()).expect("the user");
        let (pull_requests, activity) = (1..=prs).map(pull_request).unzip();
        store
            .save_fetched_pull_requests(&repository, pull_requests, activity)
            .expect("the PRs");
        let (issues, activity) = (0..ISSUES).map(issue).unzip();
        store
            .save_fetched_issues(&repository, issues, activity)
            .expect("the Issues");
        let (to_open, reader) = store::reader();
        let open = to_open.open(&path).expect("a reader");
        Self {
            folder,
            store,
            open,
            reader,
            settings: settings(&repository),
            repository,
        }
    }

    /// A database of CPython's first-sync size with what the views read beyond the facts: each starter
    /// Filter's last results, and what one Issue says.
    #[expect(
        clippy::expect_used,
        reason = "a benchmark whose database can't be built has nothing to measure"
    )]
    fn with_views(name: &str) -> Self {
        let held = Self::new(name, FIRST_SYNC_PRS);
        let open_prs: BTreeSet<u32> = (1..=FIRST_SYNC_PRS)
            .filter(|number| is_open(*number))
            .collect();
        let open_issues: BTreeSet<u32> = (0..ISSUES).map(|offset| FIRST_ISSUE + offset).collect();
        for filter in &held.settings.filters {
            let numbers = if filter.query.as_str().contains("is:pr") {
                open_prs.clone()
            } else {
                open_issues.clone()
            };
            let results = FilterResults {
                numbers,
                problem: None,
            };
            held.store
                .save_filter_results(&held.repository, &filter.query, results)
                .expect("a Filter's results");
        }
        held.store
            .save_issue_content(&held.repository, VIEWED_ISSUE, content())
            .expect("what the Issue says");
        held.store
            .save_issue_content(&held.repository, VIEWED_PR, content())
            .expect("what the PR says");
        held
    }

    fn close(self) {
        self.open.close();
        self.store.close();
        // A folder left behind sits in the ignored `.temp`, and the next run gets a new one.
        drop(fs::remove_dir_all(&self.folder));
    }
}

fn viewer() -> Login {
    Login::new("reviewer")
}

/// The login of one of `count` people, picked by `number`.
fn someone(role: &str, number: u32, count: u32) -> Login {
    Login::new(&format!("{role}-{}", number % count))
}

fn updated(number: u32) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::days(20_000) - Duration::minutes(i64::from(number))
}

/// A commit's SHA, made from `seed`.
#[expect(
    clippy::expect_used,
    reason = "a benchmark that quietly lost its commits would measure a lighter database than CPython's"
)]
fn sha(seed: u64) -> CommitSha {
    CommitSha::parse(&format!("{seed:040x}")).expect("40 hex digits")
}

/// PR `number` and what happened on it. Two in five are open, as many merged, and one closed. Every sixth
/// is a backport, and the user wrote, reviewed or was asked to review a few.
fn pull_request(number: u32) -> (PullRequestFacts, ItemActivity) {
    let state = if is_open(number) {
        PrState::Open
    } else if number % 5 < 4 {
        PrState::Merged
    } else {
        PrState::Closed
    };
    let author = if number.is_multiple_of(50) {
        viewer()
    } else {
        someone("contributor", number, 400)
    };
    let mut review_requests = vec![someone("core-dev", number, 30)];
    if number.is_multiple_of(20) {
        review_requests.push(viewer());
    }
    let mut latest_reviews = vec![LatestReview {
        author: someone("core-dev", number + 7, 30),
        state: ReviewState::Commented,
    }];
    if number.is_multiple_of(25) {
        latest_reviews.push(LatestReview {
            author: viewer(),
            state: ReviewState::Approved,
        });
    }
    let off_default = (number.is_multiple_of(6)).then(|| OffDefault {
        description: format!("(cherry picked from commit {number:040x})"),
        cherry_picked_from: vec![sha(u64::from(number))],
        committers: vec![Some(Login::new("miss-islington"))],
    });
    let facts = PullRequestFacts {
        number,
        title: format!("gh-{}: Fix the thing", FIRST_ISSUE + number % ISSUES),
        merge_commit: (state == PrState::Merged).then(|| sha(u64::from(number) << 8)),
        base: None,
        head: None,
        state,
        is_draft: number.is_multiple_of(9),
        author: Some(author.clone()),
        target_branch: if off_default.is_some() {
            "3.14"
        } else {
            "main"
        }
        .to_owned(),
        ci: Some(CiState::Success),
        updated_at: updated(number),
        review_requests,
        latest_reviews,
        assignees: (number.is_multiple_of(4))
            .then(|| someone("core-dev", number + 3, 30))
            .into_iter()
            .collect(),
        closing_issues: vec![FIRST_ISSUE + number % ISSUES],
        off_default,
    };
    (facts, events(number, &author))
}

fn issue(offset: u32) -> (IssueFacts, ItemActivity) {
    let number = FIRST_ISSUE + offset;
    let author = someone("contributor", number, 400);
    let facts = IssueFacts {
        number,
        title: format!("Something is wrong {offset}"),
        author: Some(author.clone()),
        assignees: (offset.is_multiple_of(5))
            .then(|| someone("core-dev", offset, 30))
            .into_iter()
            .collect(),
        updated_at: updated(offset),
        state: Some(IssueState::Open),
    };
    (facts, events(number, &author))
}

fn is_open(number: u32) -> bool {
    number % 5 < 2
}

/// What a busy Issue says: a description and the 20 latest comments, as GitHub renders them.
fn content() -> IssueContent {
    let paragraph = "<p>It crashes when the <code>gc</code> runs during <code>__del__</code>.</p>";
    IssueContent {
        body_html: paragraph.repeat(20),
        comments: (0..20)
            .map(|at| IssueComment {
                author: Some(someone("core-dev", at, 30)),
                created_at: updated(at),
                body_html: paragraph.repeat(3),
                hidden: None,
            })
            .collect(),
        earlier_comments: 40,
    }
}

/// A few comments on item `number`, one of them by its `author`.
fn events(number: u32, author: &Login) -> ItemActivity {
    let at = updated(number);
    let comment = |who: Login, minutes: i64| Activity {
        kind: ActivityKind::Comment,
        who,
        at: at - Duration::minutes(minutes),
    };
    ItemActivity {
        number,
        events: vec![
            comment(author.clone(), 30),
            comment(someone("core-dev", number, 30), 20),
            comment(someone("contributor", number + 1, 400), 10),
        ],
    }
}
