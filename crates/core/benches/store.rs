//! The store's reads, on a database sized like CPython's (ADR 0017).

use std::fs;
use std::path::{Path, PathBuf};

use time::{Duration, OffsetDateTime};
use tsugiki_core::github::activity::{Activity, ActivityKind, ItemActivity};
use tsugiki_core::github::issues::IssueFacts;
use tsugiki_core::github::pull_requests::{
    CiState, LatestReview, OffDefault, PrState, PullRequestFacts, ReviewState,
};
use tsugiki_core::github::{CommitSha, Login};
use tsugiki_core::repository::RepositoryName;
use tsugiki_core::store::{self, OpenReader, Store, StoreReader};

fn main() {
    divan::main();
}

/// The PRs a first sync of CPython held on 2026-11-05 (ADR 0015), and five times as many, for a store that
/// has kept months of refreshes and opened trees.
const PRS: [u32; 2] = [3_631, 18_155];

/// The Issues that same first sync held.
const ISSUES: u32 = 2_368;

/// The first Issue's number, above every PR's, so the two never share one.
const FIRST_ISSUE: u32 = 200_000;

#[divan::bench(args = PRS)]
fn work_facts(bencher: divan::Bencher<'_, '_>, prs: u32) {
    let held = Held::new(prs);
    bencher.bench(|| held.reader.work_facts(&held.repository));
    held.close();
}

/// A database of CPython's size, open for writing and for reading.
struct Held {
    folder: PathBuf,
    store: Store,
    open: OpenReader,
    reader: StoreReader,
    repository: RepositoryName,
}

impl Held {
    #[expect(
        clippy::expect_used,
        reason = "a benchmark whose database can't be built has nothing to measure"
    )]
    fn new(prs: u32) -> Self {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.temp")
            .join(format!("bench-store-{prs}-{}", std::process::id()));
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
            repository,
        }
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
    let state = match number % 5 {
        0 | 1 => PrState::Open,
        2 | 3 => PrState::Merged,
        _ => PrState::Closed,
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
    };
    (facts, events(number, &author))
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
