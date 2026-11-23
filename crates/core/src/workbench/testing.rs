//! Test helpers for the Workbench: facts to read, and an outline of what they show.

use std::collections::{BTreeMap, BTreeSet};

use time::OffsetDateTime;

use super::outcomes::Place;
use super::{
    Authored, IssueItem, ListedItem, MuteAuthor, Outcome, Outcomes, TreeEntry, Workbench,
    WorkbenchItem, WorkbenchRow, WorkbenchView, authored_by, candidates_of,
};
use crate::conventions::Conventions;
use crate::facts::activity::{Activity, ActivityFacts, ActivityKind, ItemKey, PutAway};
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::{CommitPatches, FilePatch, PatchId, PatchesOf, ReviewFacts};
use crate::facts::pull_request_content::{PullRequestContent, Review};
use crate::facts::pull_requests::{
    LatestReview, OffDefault, PrState, PullRequestFacts, ReviewState,
};
use crate::facts::{CommitSha, WorkFacts};
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::session::keep_checks;
use crate::store::testing::OpenStore;
use crate::store::{Archiving, AuthorMuting, Flagging, Muting, Seeing};
use crate::testing::{
    cpython, cpython_settings, empty_conversation, hours_ago, issue_facts, pr_facts, sha_of,
};
use crate::work::{Causes, SeenMark, WorkReason, reasons};

pub(crate) use crate::testing::me;

impl ListedItem {
    pub(crate) fn is_archived(&self) -> bool {
        self.hidden == Some(super::Hidden::Archived)
    }
}

/// A user action that can finish an item, as `Outcomes` names each one (#1052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Doing {
    MarkSeen,
    Archive,
    Unarchive,
    MarkUnseen,
    Mute,
    MuteAuthor,
    Pin,
    Unpin,
}

impl Doing {
    pub(crate) const ALL: [Self; 8] = [
        Self::MarkSeen,
        Self::Archive,
        Self::Unarchive,
        Self::MarkUnseen,
        Self::Mute,
        Self::MuteAuthor,
        Self::Pin,
        Self::Unpin,
    ];

    /// The outcome `outcomes` give this action.
    pub(crate) fn in_outcomes(self, outcomes: &Outcomes) -> Outcome {
        match self {
            Self::MarkSeen => outcomes.mark_seen,
            Self::Archive => outcomes.archive,
            Self::Unarchive => outcomes.unarchive,
            Self::MarkUnseen => outcomes.mark_unseen,
            Self::Mute => outcomes.mute,
            Self::MuteAuthor => outcomes.mute_author,
            Self::Pin => outcomes.pin,
            Self::Unpin => outcomes.unpin,
        }
    }

    /// Whether the window offers this action on `listed`, as its keys and buttons do: Archive and Mute on
    /// no Muted item (#492, #494), Unarchive on an Archived one (#491), Mute author where the item's author
    /// isn't Muted and isn't the user, and a pin or an unpin on an Issue as it stands (#488).
    fn offered(self, listed: &ListedItem) -> bool {
        let muted = listed.hidden.is_some_and(super::Hidden::is_muted);
        let issue = matches!(listed.item, WorkbenchItem::Issue(_));
        match self {
            Self::MarkSeen | Self::MarkUnseen => true,
            Self::Archive | Self::Mute => !muted,
            Self::Unarchive => listed.is_archived(),
            Self::MuteAuthor => matches!(listed.item.own_mute_author(), MuteAuthor::Mute { .. }),
            Self::Pin => issue && !listed.item.is_pinned(),
            Self::Unpin => issue && listed.item.is_pinned(),
        }
    }
}

impl ViewWorld {
    /// `doing` on `listed`, as the window draws it, through the store as the session writes it: Mark Seen
    /// on an Archived item archives it again, and Mute author marks each open item of the author's seen.
    pub(crate) fn act(&self, listed: &ListedItem, doing: Doing) {
        let store = &self.db.store;
        let repository = cpython();
        let now = OffsetDateTime::now_utc();
        let key = listed.key;
        let SeenMark {
            numbers,
            up_to,
            findings,
        } = listed.item.seen().mark.clone();
        let archiving = |put_away| Archiving {
            key,
            numbers: numbers.clone(),
            up_to,
            put_away,
            findings: findings.clone(),
        };
        let done = match doing {
            Doing::MarkSeen if listed.is_archived() => {
                store.archive(&repository, archiving(PutAway::AsBefore), now)
            }
            Doing::MarkSeen => store.mark_seen(
                &repository,
                Seeing {
                    numbers,
                    up_to,
                    findings,
                },
            ),
            Doing::Archive => store.archive(&repository, archiving(PutAway::Now), now),
            Doing::Unarchive => store.unarchive(&repository, key),
            Doing::MarkUnseen => {
                let flagging = Flagging {
                    number: key.number(),
                    archived: listed.is_archived().then_some(key),
                };
                store.mark_unseen(&repository, flagging, now)
            }
            Doing::Mute => {
                let muting = Muting {
                    key,
                    numbers,
                    up_to,
                    findings,
                };
                store.mute(&repository, muting, now)
            }
            Doing::MuteAuthor => {
                self.mute_author_as_drawn(listed.item.mute_author_offered().unwrap());
                Ok(())
            }
            Doing::Pin => store.pin(&repository, key.number()),
            Doing::Unpin => store.unpin(&repository, key.number(), now),
        };
        done.unwrap();
    }

    /// Mute author on `author` as the session writes it, with Mark Seen on each open item they opened.
    pub(crate) fn mute_author_as_drawn(&self, author: &Login) {
        let store = &self.db.store;
        let facts = store.work_facts(&cpython()).unwrap();
        let Authored { marks, .. } = authored_by(facts, &self.settings(), author);
        let muting = AuthorMuting {
            author: author.clone(),
            marks,
        };
        (store.mute_author(&cpython(), muting, OffsetDateTime::now_utc())).unwrap();
    }

    /// Each item `view` lists, as the store holds it now, with the place it gives the item.
    pub(crate) fn placed_in(&self, view: &WorkbenchView) -> Vec<(ListedItem, Place)> {
        let order = crate::settings::ListOrder::Attention;
        let read = Workbench::read(&self.db.reader, &self.settings(), view, order).unwrap();
        let listed = read.items.into_iter().map(|listed| (listed, Place::Listed));
        let folded = read.muted.into_iter().map(|listed| (listed, Place::Folded));
        listed.chain(folded).collect()
    }
}

/// An action whose outcome `outcomes_done` holds against the store: one of `Doing` on a listed item, or
/// Mute author on the author of the row of PR `number` in the item's tree.
#[derive(Debug, Clone, Copy)]
enum Check {
    Item(Doing),
    Row(u32),
}

impl Check {
    /// The outcome `listed` gives the action.
    fn outcome(self, listed: &ListedItem) -> Outcome {
        match self {
            Self::Item(doing) => doing.in_outcomes(&listed.outcomes),
            Self::Row(number) => (listed.mute_author_rows.iter())
                .find(|row| row.number == number)
                .map_or(Outcome::Unchanged, |row| row.mute_author),
        }
    }

    /// Whether the window offers the action on `listed`: Mute author on a row where the PR's author isn't
    /// Muted, the user or the item's own author, whose outcome the item's own carries.
    fn offered(self, listed: &ListedItem) -> bool {
        match self {
            Self::Item(doing) => doing.offered(listed),
            Self::Row(number) => {
                let rows = listed.item.rows();
                let row = rows.iter().find(|row| row.number == number);
                let own = match listed.item.own_mute_author() {
                    MuteAuthor::Mute { login } => Some(login),
                    MuteAuthor::Off | MuteAuthor::Unmute { .. } => None,
                };
                row.is_some_and(|row| match &row.mute_author {
                    MuteAuthor::Mute { login } => own != Some(login),
                    MuteAuthor::Off | MuteAuthor::Unmute { .. } => false,
                })
            }
        }
    }

    /// Whether `listed` still shows what the action is on.
    fn shows(self, listed: &ListedItem) -> bool {
        match self {
            Self::Item(_) => true,
            Self::Row(number) => listed.item.rows().iter().any(|row| row.number == number),
        }
    }

    /// The action on `listed`, as the window draws it, through `world`'s store.
    fn take(self, world: &ViewWorld, listed: &ListedItem) {
        match self {
            Self::Item(doing) => world.act(listed, doing),
            Self::Row(number) => {
                let rows = listed.item.rows();
                let row = rows.iter().find(|row| row.number == number).unwrap();
                let MuteAuthor::Mute { login } = &row.mute_author else {
                    panic!("no Mute author on PR {number}");
                };
                world.mute_author_as_drawn(login);
            }
        }
    }

    fn doing(self) -> Doing {
        match self {
            Self::Item(doing) => doing,
            Self::Row(_) => Doing::MuteAuthor,
        }
    }
}

/// The outcome a list gives each action the window offers on each item it lists in the world `build` makes,
/// and Mute author on each row of its tree, each held against the list once a world built afresh takes the
/// action through the store. Each action the window doesn't offer comes Unchanged. `read` gives each item
/// the list holds, with the place it gives the item. The world of each action takes `name` with the item's
/// number and the action. A row's outcome comes with the PR's number.
pub(crate) fn outcomes_done(
    name: &str,
    build: &dyn Fn(&ViewWorld),
    read: &dyn Fn(&ViewWorld) -> Vec<(ListedItem, Place)>,
) -> Vec<(u32, Doing, Outcome)> {
    let world = ViewWorld::new(name);
    build(&world);
    let listed = read(&world);
    world.finish();
    let mut judged = Vec::new();
    for (item, _) in &listed {
        let number = item.key.number();
        let rows = (item.item.rows().into_iter()).map(|row| (row.number, Check::Row(row.number)));
        let checks = Doing::ALL.map(|doing| (number, Check::Item(doing)));
        for (on, check) in checks.into_iter().chain(rows) {
            if !check.offered(item) {
                let outcome = check.outcome(item);
                assert_eq!(
                    outcome,
                    Outcome::Unchanged,
                    "{check:?} on {number} in {name}"
                );
                continue;
            }
            // The world built afresh holds its own moments, so the item and its mark are drawn there.
            let world = ViewWorld::new(&format!("{name}-{number}-{check:?}"));
            build(&world);
            let place = |world: &ViewWorld| {
                (read(world).into_iter()).find(|(listed, _)| listed.key == item.key)
            };
            let (drawn, before) = place(&world).unwrap();
            let outcome = check.outcome(&drawn);
            check.take(&world, &drawn);
            let after = place(&world).filter(|(listed, _)| check.shows(listed));
            world.finish();
            let done = Outcome::between(Some(before), after.map(|(_, place)| place));
            assert_eq!(outcome, done, "{check:?} on {number} in {name}");
            judged.push((on, check.doing(), outcome));
        }
    }
    judged
}

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

    /// Each event, one that names the user as its kind says, happened on Issue or PR `number` at its moment.
    pub(crate) fn happened(&self, number: u32, events: &[(ActivityKind, OffsetDateTime)]) {
        let named = |(kind, at): &(ActivityKind, OffsetDateTime)| Activity {
            kind: kind.clone(),
            who: me(),
            at: *at,
        };
        self.db.happened(number, events.iter().map(named).collect());
    }

    /// The user reviewed `pull_request` at commit 1, and a push three hours ago moved it on to commit 2, a
    /// change of its own, which no check has kept a finding of yet.
    pub(crate) fn reviewed_and_pushed(&self, pull_request: PullRequestFacts) {
        let number = pull_request.number;
        let reviewed = PullRequestFacts {
            latest_reviews: vec![LatestReview {
                author: me(),
                state: ReviewState::Commented,
            }],
            ..pull_request
        };
        self.db.reviewed_and_moved(reviewed, ('a', 'b'));
        self.happened(number, &[(ActivityKind::Commit, hours_ago(3))]);
    }

    /// Mute on Issue `number`, whose tree holds `numbers`, from now.
    pub(crate) fn mute_issue(&self, number: u32, numbers: Vec<u32>) {
        let muting = Muting {
            key: ItemKey::Issue { number },
            numbers,
            up_to: None,
            findings: vec![],
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
            findings: vec![],
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
    let items = candidates_of(&facts, settings, &BTreeSet::new(), &causes);
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

/// `Lib/ast.py` changed in the way `digit` names.
fn ast_changed(digit: char) -> Vec<FilePatch> {
    let id = PatchId::parse(&digit.to_string().repeat(40)).unwrap();
    vec![FilePatch {
        path: "Lib/ast.py".to_owned(),
        verbatim: id.clone(),
        loose: id,
        generated: false,
    }]
}

/// `commit` against `main`.
pub(crate) fn on_main(commit: CommitSha) -> PatchesOf {
    PatchesOf {
        commit,
        target_branch: "main".to_owned(),
    }
}

impl OpenStore {
    /// The user reviewed PR `number` at commit 1, which has moved on to commit 2 since, and the check has
    /// read neither.
    pub(crate) fn reviewed_and_moved_unread(&self, pull_request: PullRequestFacts) {
        let number = pull_request.number;
        let moved = PullRequestFacts {
            head: Some(sha_of('2')),
            ..pull_request
        };
        self.store
            .save_pull_requests(&cpython(), vec![moved])
            .unwrap();
        let content = PullRequestContent {
            conversation: empty_conversation(),
            reviews: vec![Review {
                id: None,
                author: Some(me()),
                state: ReviewState::Commented,
                commit: Some(sha_of('1')),
                submitted_at: hours_ago(50),
                body_html: String::new(),
                hidden: None,
                threads: vec![],
            }],
            earlier_reviews: 0,
            head: Some(sha_of('2')),
            pending: vec![],
        };
        let saved = (content, OffsetDateTime::UNIX_EPOCH);
        self.store
            .save_pull_request_content(&cpython(), number, saved)
            .unwrap();
    }

    /// The user reviewed PR `number` at commit 1, which has moved on to commit 2 since, and the check read
    /// the change at commit 1 as `reviewed` and at commit 2 as `head`.
    pub(crate) fn reviewed_and_moved(
        &self,
        pull_request: PullRequestFacts,
        (reviewed, head): (char, char),
    ) {
        self.reviewed_and_moved_unread(pull_request);
        self.check_read([(sha_of('1'), reviewed), (sha_of('2'), head)]);
    }

    /// The check read the change at each commit of `read` in the way its digit names.
    pub(crate) fn check_read<const N: usize>(&self, read: [(CommitSha, char); N]) {
        let patches = read.map(|(commit, digit)| CommitPatches {
            of: on_main(commit),
            files: ast_changed(digit),
        });
        self.store
            .save_patch_ids(&cpython(), patches.to_vec())
            .unwrap();
    }

    /// The session keeps what the checks found, as it does after each read and sync, at `now`.
    pub(crate) fn checks_kept(&self, now: OffsetDateTime) {
        let facts = self.store.work_facts(&cpython()).unwrap();
        keep_checks(&self.store, &cpython(), &facts, now).unwrap();
    }
}
