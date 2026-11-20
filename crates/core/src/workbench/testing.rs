//! Test helpers for the Workbench: facts to read, and an outline of what they show.

use std::collections::{BTreeMap, BTreeSet};

use time::OffsetDateTime;

use super::{
    IssueItem, ListedItem, TreeEntry, Workbench, WorkbenchItem, WorkbenchRow, WorkbenchView,
    candidates_of,
};
use crate::conventions::Conventions;
use crate::facts::activity::{ActivityFacts, ItemKey, PutAway};
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{OffDefault, PrState, PullRequestFacts};
use crate::facts::{CommitSha, WorkFacts};
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::testing::OpenStore;
use crate::store::{Archiving, Flagging, Muting};
use crate::testing::{cpython, cpython_settings, issue_facts, pr_facts};
use crate::work::{Causes, WorkReason, reasons};

pub(crate) use crate::testing::me;

pub(crate) fn someone() -> Login {
    Login::new("some-author")
}

/// A PR that has nothing to do with the viewer.
pub(crate) fn stranger() -> PullRequestFacts {
    PullRequestFacts {
        title: "gh-1: Fix it".to_owned(),
        author: Some(someone()),
        updated_at: at(1),
        ..pr_facts(1)
    }
}

pub(crate) fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap()
}

pub(crate) fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        author: Some(Login::new("issue-author")),
        updated_at: at(1),
        ..issue_facts(number)
    }
}

/// Issue `number`, open and assigned to the user.
pub(crate) fn assigned_issue(number: u32) -> IssueFacts {
    IssueFacts {
        assignees: vec![me()],
        ..issue(number)
    }
}

/// The user's PR, whose title names no Issue.
pub(crate) fn mine(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        author: Some(me()),
        ..stranger()
    }
}

/// The user's PR, whose title names `issue`.
pub(crate) fn fixing(number: u32, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{issue}: Fix it"),
        ..mine(number)
    }
}

pub(crate) fn reasons_for(facts: &PullRequestFacts) -> Vec<WorkReason> {
    reasons(facts, &me())
}

pub(crate) fn workbench(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
) -> Workbench {
    reviewed_workbench(pull_requests, issues, ReviewFacts::default())
}

/// The view `view` of the Workbench of `facts`.
pub(crate) fn view_of(facts: WorkFacts, view: &WorkbenchView) -> Workbench {
    Workbench::from_facts(facts, &cpython_settings(), view)
}

/// Each PR `item` shows, in the order the list shows them.
pub(crate) fn rows_of(item: &WorkbenchItem) -> Vec<WorkbenchRow> {
    item.rows().into_iter().cloned().collect()
}

/// The Workbench of `pull_requests` and `issues`, with what `reviews` says of the user's Reviews.
pub(crate) fn reviewed_workbench(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
    reviews: ReviewFacts,
) -> Workbench {
    let facts = WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues,
        filters: BTreeMap::new(),
        activity: ActivityFacts::default(),
        reviews,
    };
    Workbench::from_facts(facts, &cpython_settings(), &WorkbenchView::Whole)
}

/// Each item of `workbench` as its kind and number, with the PRs it shows after it.
pub(crate) fn outline(workbench: &Workbench) -> Vec<(&'static str, u32, Vec<u32>)> {
    outline_of(&workbench.items)
}

/// Each of `items` as its kind and number, with the PRs it shows after it.
pub(crate) fn outline_of(items: &[ListedItem]) -> Vec<(&'static str, u32, Vec<u32>)> {
    items
        .iter()
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

pub(crate) fn first_issue(workbench: &Workbench) -> &IssueItem {
    let mut issues = workbench
        .items
        .iter()
        .filter_map(|listed| match &listed.item {
            WorkbenchItem::Issue(issue) => Some(issue),
            WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => None,
        });
    issues.next().unwrap()
}

pub(crate) const BOT: &str = "miss-islington";

/// A PR on 3.14 by someone else, titled `title` and committed by `committers`.
pub(crate) fn backport(
    number: u32,
    title: &str,
    committers: Vec<Option<Login>>,
) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: title.to_owned(),
        target_branch: "3.14".to_owned(),
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: vec![],
            committers,
        }),
        ..stranger()
    }
}

pub(crate) fn bots_backport(number: u32, title: &str) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(Login::new(BOT)),
        ..backport(number, title, vec![Some(Login::new(BOT))])
    }
}

pub(crate) fn own(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(me()),
        ..facts
    }
}

/// A database with the user signed in, and a reader of it, for the Detail pane's views.
pub(crate) struct ViewWorld {
    pub(crate) db: OpenStore,
}

impl ViewWorld {
    pub(crate) fn new(name: &str) -> Self {
        let db = OpenStore::new(name);
        db.store.remember_login(&me()).unwrap();
        Self { db }
    }

    pub(crate) fn settings(&self) -> RepositorySettings {
        cpython_settings()
    }

    pub(crate) fn has(&self, pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) {
        let store = &self.db.store;
        store.save_pull_requests(&cpython(), pull_requests).unwrap();
        store.save_issues(&cpython(), issues).unwrap();
    }

    /// Mute on Issue `number`, whose tree holds `numbers`, from now.
    pub(crate) fn mute_issue(&self, number: u32, numbers: Vec<u32>) {
        let muting = Muting {
            key: ItemKey::Issue { number },
            numbers,
            up_to: None,
        };
        let now = OffsetDateTime::now_utc();
        self.db.store.mute(&cpython(), muting, now).unwrap();
    }

    /// Issues 1 to 3, and PRs whose links cross between them: Backports beneath an Origin PR that fix
    /// another Issue than it, a Backport by cherry-pick, a Backport of a Backport, a Backport whose Origin PR
    /// Tsugiki lacks, a Backport of one of those that fixes an Issue, which leaves the one it names a
    /// Standalone backport, a PR that fixes two Issues, a Standalone PR a Backport puts under an Issue, and
    /// one that stays on its own. Each PR has an update of its own. Issue 1 is archived, and PR 30 in its
    /// tree is marked unseen since, which only a read of its whole tree sees.
    pub(crate) fn has_crossed_links(&self) {
        let merged = |facts: PullRequestFacts| PullRequestFacts {
            state: PrState::Merged,
            merge_commit: CommitSha::parse(&format!("{:040x}", facts.number)),
            ..facts
        };
        let picked = PullRequestFacts {
            off_default: Some(OffDefault {
                description: format!("(cherry picked from commit {:040x})", 10),
                cherry_picked_from: CommitSha::parse(&format!("{:040x}", 10))
                    .into_iter()
                    .collect(),
                committers: vec![Some(someone())],
            }),
            ..backport(13, "Fix it on 3.12", vec![])
        };
        let both = PullRequestFacts {
            closing_issues: vec![3],
            ..fixing(30, 1)
        };
        let pull_requests = vec![
            merged(fixing(10, 1)),
            bots_backport(11, "[3.14] gh-1: Fix it (GH-10)"),
            backport(12, "[3.13] gh-2: Fix it (GH-10)", vec![]),
            picked,
            bots_backport(14, "[3.12] gh-1: Fix it (GH-11)"),
            own(fixing(20, 2)),
            both,
            stranger_numbered(40),
            bots_backport(41, "[3.14] gh-3: Fix it (GH-40)"),
            backport(50, "[3.14] gh-1: Fix it (GH-99)", vec![]),
            backport(51, "[3.13] Fix it (GH-98)", vec![]),
            bots_backport(52, "[3.12] gh-2: Fix it (GH-51)"),
            stranger_numbered(60),
        ];
        let updated =
            (pull_requests.into_iter().zip(1..)).map(|(facts, seconds)| PullRequestFacts {
                updated_at: at(seconds),
                ..facts
            });
        self.has(updated.collect(), vec![issue(1), issue(2), issue(3)]);
        let archiving = Archiving {
            key: ItemKey::Issue { number: 1 },
            numbers: vec![1],
            up_to: None,
            put_away: PutAway::Now,
        };
        let now = OffsetDateTime::now_utc();
        let store = &self.db.store;
        store.archive(&cpython(), archiving, now).unwrap();
        store
            .mark_unseen(&cpython(), Flagging::on(30), now)
            .unwrap();
    }

    pub(crate) fn finish(self) {
        self.db.close();
    }
}

/// A PR numbered `number` that names no Issue and has nothing to do with the viewer.
fn stranger_numbered(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        ..stranger()
    }
}

/// Each item the grouping of every Issue and PR the store holds gives under `settings`, and the causes it
/// judged them by: what the Issue view and the PR view read before they read one tree (#842).
pub(crate) fn grouped_whole(
    world: &ViewWorld,
    settings: &RepositorySettings,
) -> (Vec<WorkbenchItem>, Causes) {
    let facts = world.db.reader.work_facts(&settings.name).unwrap();
    let causes = Causes::of(&facts, settings.conventions);
    let items = candidates_of(facts, settings, &BTreeSet::new(), &causes);
    (items, causes)
}

/// `settings` under each Conventions.
pub(crate) fn under_each(settings: &RepositorySettings) -> Vec<RepositorySettings> {
    let under = |conventions| RepositorySettings {
        conventions,
        ..settings.clone()
    };
    vec![under(Conventions::Cpython), under(Conventions::Github)]
}
