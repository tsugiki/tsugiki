//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod filters;

pub use filters::FilterLine;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::error::Error;
use crate::filters::FilterQuery;
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{CiState, PrState, PullRequestFacts, ReviewState};
use crate::links::{BackportKind, BackportLink, MergeCommits, backport_of, fixed_numbers};
use crate::repository::RepositorySettings;
use crate::store::{StoreReader, WorkFacts};
use filters::FilterFacts;

/// The reason a PR is on the user's Workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkReason {
    /// The user is a requested reviewer or an assignee.
    ReviewRequest,
    OwnPr,
    /// The user has submitted a Review.
    Reviewed,
}

/// A PR's line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WorkbenchRow {
    pub number: u32,
    pub title: String,
    pub author: Option<Login>,
    pub state: PrState,
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    /// Empty for a PR that another PR put on the Workbench, and for one no longer open.
    pub reasons: Vec<WorkReason>,
}

/// A Backport's line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BackportRow {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    pub backport: BackportKind,
}

/// A PR with its Backports beneath it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrTree {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    pub backports: Vec<BackportRow>,
}

/// A line directly under an Issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TreeEntry {
    Pr(PrTree),
    /// A Backport whose Origin PR Tsugiki can't show: that number names no PR, or a Backport.
    Backport(BackportRow),
}

impl TreeEntry {
    fn rows(&self) -> Vec<&WorkbenchRow> {
        match self {
            Self::Pr(tree) => tree.rows(),
            Self::Backport(backport) => vec![&backport.row],
        }
    }
}

impl PrTree {
    fn rows(&self) -> Vec<&WorkbenchRow> {
        let beneath = self.backports.iter().map(|backport| &backport.row);
        std::iter::once(&self.row).chain(beneath).collect()
    }
}

/// An Issue on the Workbench, with its tree: each PR Tsugiki knows that fixes it, whatever its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueItem {
    pub number: u32,
    pub title: String,
    /// The Issue's author, then the PRs' authors, with each person once and no backport bot.
    pub people: Vec<Login>,
    /// The latest update of the Issue and its PRs.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    pub pull_requests: Vec<TreeEntry>,
}

/// A top-level entry of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchItem {
    Issue(IssueItem),
    /// A PR that fixes no Issue, with its Backports.
    StandalonePr(PrTree),
    /// A Backport with no Issue and no Origin PR to sit under.
    StandaloneBackport(BackportRow),
}

impl WorkbenchItem {
    fn updated_at_and_number(&self) -> (OffsetDateTime, u32) {
        match self {
            Self::Issue(issue) => (issue.updated_at, issue.number),
            Self::StandalonePr(tree) => (tree.row.updated_at, tree.row.number),
            Self::StandaloneBackport(backport) => (backport.row.updated_at, backport.row.number),
        }
    }

    fn rows(&self) -> Vec<&WorkbenchRow> {
        match self {
            Self::Issue(issue) => issue
                .pull_requests
                .iter()
                .flat_map(TreeEntry::rows)
                .collect(),
            Self::StandalonePr(tree) => tree.rows(),
            Self::StandaloneBackport(backport) => vec![&backport.row],
        }
    }
}

/// A Repository's Workbench or one of its Filters, with what the Sidebar shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    /// The selected items, the latest update first.
    pub items: Vec<WorkbenchItem>,
    /// How many items the Workbench holds, until Needs attention exists to count instead.
    pub count: u32,
    pub filters: Vec<FilterLine>,
}

impl Workbench {
    /// Reads the Workbench of `repository`, or the Filter `selected`. This waits on the database and never on
    /// a sync (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        selected: Option<&FilterQuery>,
    ) -> Result<Self, Error> {
        let facts = reader.work_facts(&repository.name)?;
        Ok(Self::from_facts(facts, repository, selected))
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's.
    fn from_facts(
        mut facts: WorkFacts,
        repository: &RepositorySettings,
        selected: Option<&FilterQuery>,
    ) -> Self {
        let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
        let matched = filters.matched();
        let selected = selected.map(|query| filters.numbers_of(query));
        let candidates = candidates(facts, repository, selected.unwrap_or(&matched));
        let on_workbench =
            |item: &&WorkbenchItem| any_reason(&item.rows()) || item.is_reached_by(&matched);
        let lines = filters.lines(&candidates);
        let count = count_of(candidates.iter().filter(on_workbench));
        let mut items: Vec<WorkbenchItem> = candidates
            .iter()
            .filter(|item| match selected {
                Some(numbers) => item.is_reached_by(numbers),
                None => on_workbench(item),
            })
            .cloned()
            .collect();
        items.sort_by_key(|item| Reverse(item.updated_at_and_number()));
        Self {
            items,
            count,
            filters: lines,
        }
    }
}

fn count_of<'a>(items: impl Iterator<Item = &'a WorkbenchItem>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// Each item the Repository's facts could show, on the Workbench or not. A PR in `first`, the Matched PRs of
/// the view on show, comes first in its Issue's tree.
fn candidates(
    facts: WorkFacts,
    repository: &RepositorySettings,
    first: &BTreeSet<u32>,
) -> Vec<WorkbenchItem> {
    let Some(viewer) = facts.viewer else {
        return Vec::new();
    };
    let linked = link(facts.pull_requests, &facts.issues, repository, &viewer);
    let mut issues: BTreeMap<u32, (IssueFacts, Vec<TreeEntry>)> = facts
        .issues
        .into_iter()
        .map(|issue| (issue.number, (issue, Vec::new())))
        .collect();
    let mut items = Vec::new();
    for top in plant(linked) {
        if top.issues.is_empty() {
            items.push(standalone(top));
            continue;
        }
        for number in &top.issues {
            if let Some((_, entries)) = issues.get_mut(number) {
                entries.push(top.entry_under(*number));
            }
        }
    }
    items.extend(issues.into_values().map(|(issue, mut entries)| {
        entries.sort_by_key(|entry| !entry.rows().iter().any(|row| first.contains(&row.number)));
        issue_item(issue, entries, repository.conventions)
    }));
    items
}

/// A PR with what the link rules say about it.
struct Linked {
    row: WorkbenchRow,
    /// The Issues it fixes. Only a number the sync fetched as an Issue counts, so a PR's number or a typo
    /// doesn't.
    issues: BTreeSet<u32>,
    backport: Option<BackportLink>,
}

fn link(
    pull_requests: Vec<PullRequestFacts>,
    issues: &[IssueFacts],
    repository: &RepositorySettings,
    viewer: &Login,
) -> Vec<Linked> {
    let merges = MergeCommits::of(&pull_requests);
    let known: BTreeSet<u32> = issues.iter().map(|issue| issue.number).collect();
    pull_requests
        .into_iter()
        .map(|facts| {
            let fixed = fixed_numbers(&facts, repository);
            Linked {
                issues: fixed.intersection(&known).copied().collect(),
                backport: backport_of(&facts, repository.conventions, &merges),
                row: row(facts, viewer),
            }
        })
        .collect()
}

/// A Backport beneath its Origin PR.
struct Beneath {
    row: BackportRow,
    issues: BTreeSet<u32>,
}

/// The two shapes of a tree's top line.
enum Shape {
    Pr {
        beneath: Vec<Beneath>,
    },
    /// A Backport with no Origin PR to sit under.
    Backport(BackportKind),
}

/// A line that sits directly under an Issue or on its own.
struct Top {
    row: WorkbenchRow,
    shape: Shape,
    /// Each Issue it shows under: its own, and those of the Backports beneath it (ADR 0011).
    issues: BTreeSet<u32>,
}

impl Top {
    /// A Backport shows under an Issue it fixes. One that fixes none follows its Origin PR.
    fn entry_under(&self, issue: u32) -> TreeEntry {
        let row = self.row.clone();
        match &self.shape {
            Shape::Backport(backport) => TreeEntry::Backport(BackportRow {
                row,
                backport: *backport,
            }),
            Shape::Pr { beneath } => TreeEntry::Pr(PrTree {
                row,
                backports: beneath
                    .iter()
                    .filter(|below| below.issues.is_empty() || below.issues.contains(&issue))
                    .map(|below| below.row.clone())
                    .collect(),
            }),
        }
    }
}

/// Puts each Backport beneath its Origin PR when Tsugiki has that PR and it isn't a Backport too, which keeps
/// a tree two levels deep. The order of `linked` stays within each level.
fn plant(linked: Vec<Linked>) -> Vec<Top> {
    let origins: BTreeSet<u32> = linked
        .iter()
        .filter(|pr| pr.backport.is_none())
        .map(|pr| pr.row.number)
        .collect();
    let mut beneath: BTreeMap<u32, Vec<Beneath>> = BTreeMap::new();
    let mut tops = Vec::new();
    for pr in linked {
        match pr.backport {
            Some(link) if origins.contains(&link.origin) => {
                beneath.entry(link.origin).or_default().push(Beneath {
                    row: BackportRow {
                        row: pr.row,
                        backport: link.kind,
                    },
                    issues: pr.issues,
                });
            }
            _ => tops.push(Top {
                row: pr.row,
                shape: pr.backport.map_or_else(
                    || Shape::Pr {
                        beneath: Vec::new(),
                    },
                    |link| Shape::Backport(link.kind),
                ),
                issues: pr.issues,
            }),
        }
    }
    for top in &mut tops {
        if let Shape::Pr { beneath: place } = &mut top.shape
            && let Some(below) = beneath.remove(&top.row.number)
        {
            top.issues
                .extend(below.iter().flat_map(|b| b.issues.iter().copied()));
            *place = below;
        }
    }
    tops
}

/// The item for a line with no Issue. Nothing beneath it fixes an Issue, or the line would show under that
/// Issue.
fn standalone(top: Top) -> WorkbenchItem {
    match top.shape {
        Shape::Backport(backport) => WorkbenchItem::StandaloneBackport(BackportRow {
            row: top.row,
            backport,
        }),
        Shape::Pr { beneath } => WorkbenchItem::StandalonePr(PrTree {
            row: top.row,
            backports: beneath.into_iter().map(|below| below.row).collect(),
        }),
    }
}

fn any_reason(rows: &[&WorkbenchRow]) -> bool {
    rows.iter().any(|row| !row.reasons.is_empty())
}

fn issue_item(
    issue: IssueFacts,
    pull_requests: Vec<TreeEntry>,
    conventions: Conventions,
) -> WorkbenchItem {
    let people = people(&issue, &pull_requests, conventions);
    let updated_at = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .map(|row| row.updated_at)
        .fold(issue.updated_at, Ord::max);
    WorkbenchItem::Issue(IssueItem {
        number: issue.number,
        title: issue.title,
        people,
        updated_at,
        pull_requests,
    })
}

fn people(issue: &IssueFacts, pull_requests: &[TreeEntry], conventions: Conventions) -> Vec<Login> {
    let authors = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .filter_map(|row| row.author.as_ref());
    let mut people: Vec<Login> = Vec::new();
    for author in issue.author.iter().chain(authors) {
        if !conventions.is_backport_bot(author) && !people.contains(author) {
            people.push(author.clone());
        }
    }
    people
}

/// The PR's line of the list, whatever it has to do with `viewer`.
fn row(facts: PullRequestFacts, viewer: &Login) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
    WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        state: facts.state,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        updated_at: facts.updated_at,
        reasons,
    }
}

/// A PR that has merged or closed waits on nobody, so it keeps no Issue on the Workbench.
pub(crate) fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
    if facts.state != PrState::Open {
        return Vec::new();
    }
    let asked = facts.review_requests.contains(viewer) || facts.assignees.contains(viewer);
    let own = facts.author.as_ref() == Some(viewer);
    // A pending Review is a draft only its author can see, so it isn't one yet.
    let reviewed = facts
        .latest_reviews
        .iter()
        .any(|review| review.author == *viewer && review.state != ReviewState::Pending);
    [
        (asked, WorkReason::ReviewRequest),
        (own, WorkReason::OwnPr),
        (reviewed, WorkReason::Reviewed),
    ]
    .into_iter()
    .filter_map(|(applies, reason)| applies.then_some(reason))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::CommitSha;
    use crate::github::pull_requests::{LatestReview, OffDefault};
    use crate::repository::RepositoryName;

    pub(super) fn me() -> Login {
        Login::new("octocat")
    }

    pub(super) fn someone() -> Login {
        Login::new("some-author")
    }

    /// A PR that has nothing to do with the viewer.
    pub(super) fn stranger() -> PullRequestFacts {
        PullRequestFacts {
            number: 1,
            title: "gh-1: Fix it".to_owned(),
            state: PrState::Open,
            is_draft: false,
            author: Some(someone()),
            target_branch: "main".to_owned(),
            ci: None,
            updated_at: at(1),
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
            closing_issues: vec![],
            merge_commit: None,
            off_default: None,
        }
    }

    fn reasons_for(facts: &PullRequestFacts) -> Vec<WorkReason> {
        reasons(facts, &me())
    }

    fn review(author: Login, state: ReviewState) -> LatestReview {
        LatestReview { author, state }
    }

    #[test]
    fn a_requested_reviewer_has_a_review_request() {
        let facts = PullRequestFacts {
            review_requests: vec![me()],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
    }

    #[test]
    fn an_assignee_has_a_review_request() {
        let facts = PullRequestFacts {
            assignees: vec![me()],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
    }

    #[test]
    fn a_pr_the_user_wrote_is_an_own_pr() {
        let facts = PullRequestFacts {
            author: Some(me()),
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), [WorkReason::OwnPr]);
    }

    #[test]
    fn a_submitted_review_counts_whatever_it_said() {
        for state in [
            ReviewState::Commented,
            ReviewState::Approved,
            ReviewState::ChangesRequested,
            ReviewState::Dismissed,
        ] {
            let facts = PullRequestFacts {
                latest_reviews: vec![review(me(), state)],
                ..stranger()
            };
            assert_eq!(reasons_for(&facts), [WorkReason::Reviewed], "{state:?}");
        }
    }

    #[test]
    fn a_pending_review_isnt_a_review_yet() {
        let facts = PullRequestFacts {
            latest_reviews: vec![review(me(), ReviewState::Pending)],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), []);
    }

    #[test]
    fn someone_elses_review_or_request_isnt_the_users() {
        let facts = PullRequestFacts {
            review_requests: vec![someone()],
            assignees: vec![someone()],
            latest_reviews: vec![review(someone(), ReviewState::Approved)],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), []);
    }

    #[test]
    fn a_pr_with_no_author_isnt_the_users() {
        let facts = PullRequestFacts {
            author: None,
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), []);
    }

    #[test]
    fn a_login_matches_whatever_its_case() {
        let facts = PullRequestFacts {
            review_requests: vec![Login::new("OctoCat")],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
    }

    #[test]
    fn every_reason_that_applies_is_listed() {
        let facts = PullRequestFacts {
            author: Some(me()),
            assignees: vec![me()],
            latest_reviews: vec![review(me(), ReviewState::Commented)],
            ..stranger()
        };
        assert_eq!(
            reasons_for(&facts),
            [
                WorkReason::ReviewRequest,
                WorkReason::OwnPr,
                WorkReason::Reviewed
            ]
        );
    }

    pub(super) fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).unwrap()
    }

    pub(super) fn cpython() -> RepositorySettings {
        RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap())
    }

    pub(super) fn issue(number: u32) -> IssueFacts {
        IssueFacts {
            number,
            title: format!("Issue {number}"),
            author: Some(Login::new("issue-author")),
            updated_at: at(1),
        }
    }

    /// The user's PR, whose title names no Issue.
    pub(super) fn mine(number: u32) -> PullRequestFacts {
        PullRequestFacts {
            number,
            title: format!("PR {number}"),
            author: Some(me()),
            ..stranger()
        }
    }

    /// The user's PR, whose title names `issue`.
    pub(super) fn fixing(number: u32, issue: u32) -> PullRequestFacts {
        PullRequestFacts {
            title: format!("gh-{issue}: Fix it"),
            ..mine(number)
        }
    }

    fn workbench(pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) -> Workbench {
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests,
            issues,
            filters: BTreeMap::new(),
        };
        Workbench::from_facts(facts, &cpython(), None)
    }

    /// Each item as its kind and number, with an Issue's PRs after it.
    pub(super) fn outline(workbench: &Workbench) -> Vec<(&'static str, u32, Vec<u32>)> {
        workbench
            .items
            .iter()
            .map(|item| match item {
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

    pub(super) fn first_issue(workbench: &Workbench) -> &IssueItem {
        let issues = workbench.items.iter().filter_map(|item| match item {
            WorkbenchItem::Issue(issue) => Some(issue),
            WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => None,
        });
        let mut issues = issues;
        issues.next().unwrap()
    }

    #[test]
    fn a_pr_naming_an_issue_sits_under_it() {
        let found = workbench(vec![fixing(10, 1)], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![10])]);
    }

    #[test]
    fn a_pr_naming_no_issue_is_a_standalone_pr() {
        let found = workbench(vec![mine(10)], vec![issue(1)]);
        assert_eq!(outline(&found), [("standalone", 10, vec![])]);
    }

    #[test]
    fn a_number_the_sync_didnt_fetch_as_an_issue_doesnt_link() {
        let found = workbench(vec![fixing(10, 9)], vec![issue(1)]);
        assert_eq!(outline(&found), [("standalone", 10, vec![])]);
    }

    #[test]
    fn changing_a_repositorys_conventions_regroups_the_same_facts() {
        let facts = || WorkFacts {
            viewer: Some(me()),
            filters: BTreeMap::new(),
            pull_requests: vec![fixing(10, 1)],
            issues: vec![issue(1)],
        };
        let github = RepositorySettings {
            conventions: Conventions::Github,
            ..cpython()
        };

        let under_cpython = Workbench::from_facts(facts(), &cpython(), None);
        let under_github = Workbench::from_facts(facts(), &github, None);

        assert_eq!(outline(&under_cpython), [("issue", 1, vec![10])]);
        assert_eq!(outline(&under_github), [("standalone", 10, vec![])]);
    }

    #[test]
    fn a_pr_fixing_several_issues_appears_under_each() {
        let both = PullRequestFacts {
            closing_issues: vec![2],
            ..fixing(10, 1)
        };
        let found = workbench(vec![both], vec![issue(1), issue(2)]);
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10]), ("issue", 1, vec![10])]
        );
    }

    #[test]
    fn a_retitled_pr_moves_to_the_issue_its_title_names_now() {
        let before = workbench(vec![fixing(10, 1)], vec![issue(1), issue(2)]);
        let after = workbench(vec![fixing(10, 2)], vec![issue(1), issue(2)]);
        assert_eq!(outline(&before), [("issue", 1, vec![10])]);
        assert_eq!(outline(&after), [("issue", 2, vec![10])]);
    }

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
    fn a_pr_of_someone_elses_with_no_issue_isnt_listed() {
        let found = workbench(vec![stranger()], vec![]);
        assert_eq!(outline(&found), []);
    }

    #[test]
    fn an_issue_lists_every_pr_fixing_it_once_one_is_the_users() {
        let other = PullRequestFacts {
            number: 11,
            title: "gh-1: Fix it too".to_owned(),
            ..stranger()
        };
        let found = workbench(vec![other, fixing(10, 1)], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![11, 10])]);
    }

    #[test]
    fn an_issues_people_are_its_author_then_its_prs_authors_once_each() {
        let issue_author = PullRequestFacts {
            number: 11,
            title: "gh-1: Fix it".to_owned(),
            author: Some(Login::new("Issue-Author")),
            ..stranger()
        };
        let bot = PullRequestFacts {
            number: 12,
            title: "[3.14] gh-1: Fix it".to_owned(),
            author: Some(Login::new("miss-islington")),
            ..stranger()
        };
        let gone = PullRequestFacts {
            number: 13,
            title: "gh-1: Fix it".to_owned(),
            author: None,
            ..stranger()
        };
        let found = workbench(vec![fixing(10, 1), issue_author, bot, gone], vec![issue(1)]);
        assert_eq!(
            first_issue(&found).people,
            [Login::new("issue-author"), me()],
        );
    }

    #[test]
    fn an_issue_with_no_author_starts_its_people_at_its_prs() {
        let gone = IssueFacts {
            author: None,
            ..issue(1)
        };
        let found = workbench(vec![fixing(10, 1)], vec![gone]);
        assert_eq!(first_issue(&found).people, [me()]);
    }

    #[test]
    fn an_issues_time_is_the_latest_of_it_and_its_prs() {
        let later_pr = PullRequestFacts {
            updated_at: at(5),
            ..fixing(10, 1)
        };
        let found = workbench(vec![later_pr.clone()], vec![issue(1)]);
        assert_eq!(first_issue(&found).updated_at, at(5));

        let later_issue = IssueFacts {
            updated_at: at(9),
            ..issue(1)
        };
        let found = workbench(vec![later_pr], vec![later_issue]);
        assert_eq!(first_issue(&found).updated_at, at(9));
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
    fn a_row_carries_what_the_list_shows() {
        let found = workbench(
            vec![PullRequestFacts {
                is_draft: true,
                ci: Some(CiState::Pending),
                updated_at: at(3),
                ..mine(3)
            }],
            vec![],
        );
        assert_eq!(
            found.items,
            [WorkbenchItem::StandalonePr(PrTree {
                row: WorkbenchRow {
                    number: 3,
                    title: "PR 3".to_owned(),
                    author: Some(me()),
                    state: PrState::Open,
                    is_draft: true,
                    target_branch: "main".to_owned(),
                    ci: Some(CiState::Pending),
                    updated_at: at(3),
                    reasons: vec![WorkReason::OwnPr],
                },
                backports: vec![],
            })]
        );
    }

    #[test]
    fn the_window_gets_the_kind_of_each_item_and_its_times_as_text() {
        let found = workbench(vec![mine(3), fixing(4, 1)], vec![issue(1)]);
        let sent = serde_json::to_value(&found).unwrap();
        assert_eq!(sent["items"][0]["kind"], "standalone_pr");
        assert_eq!(sent["items"][0]["number"], 3);
        assert_eq!(sent["items"][0]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["items"][1]["kind"], "issue");
        assert_eq!(sent["items"][1]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["items"][1]["pull_requests"][0]["kind"], "pr");
        assert_eq!(sent["items"][1]["pull_requests"][0]["number"], 4);
    }

    #[test]
    fn the_window_gets_a_backports_kind_beside_its_row() {
        let found = workbench(
            vec![
                fixing(4, 1),
                backport(5, "[3.14] gh-1: Fix it (GH-4)", vec![]),
                backport(6, "[3.13] gh-1: Fix it (GH-99)", vec![]),
            ],
            vec![issue(1)],
        );
        let sent = serde_json::to_value(&found).unwrap();
        let entries = &sent["items"][0]["pull_requests"];
        assert_eq!(entries[0]["backports"][0]["number"], 5);
        assert_eq!(entries[0]["backports"][0]["backport"], "manual");
        assert_eq!(entries[1]["kind"], "backport");
        assert_eq!(entries[1]["number"], 6);
        assert_eq!(entries[1]["backport"], "manual");
    }

    const BOT: &str = "miss-islington";

    /// A PR on 3.14 by someone else, titled `title` and committed by `committers`.
    pub(super) fn backport(
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

    pub(super) fn bots_backport(number: u32, title: &str) -> PullRequestFacts {
        PullRequestFacts {
            author: Some(Login::new(BOT)),
            ..backport(number, title, vec![Some(Login::new(BOT))])
        }
    }

    fn own(facts: PullRequestFacts) -> PullRequestFacts {
        PullRequestFacts {
            author: Some(me()),
            ..facts
        }
    }

    /// The first Issue's lines, as each PR's number with the numbers beneath it.
    pub(super) fn tree_of(workbench: &Workbench) -> Vec<(u32, Vec<u32>)> {
        first_issue(workbench)
            .pull_requests
            .iter()
            .map(|entry| match entry {
                TreeEntry::Pr(tree) => (
                    tree.row.number,
                    tree.backports
                        .iter()
                        .map(|below| below.row.number)
                        .collect(),
                ),
                TreeEntry::Backport(backport) => (backport.row.number, vec![]),
            })
            .collect()
    }

    #[test]
    fn a_backport_sits_under_its_origin_pr_with_its_kind() {
        let found = workbench(
            vec![
                fixing(10, 1),
                bots_backport(11, "[3.14] gh-1: Fix it (GH-10)"),
                backport(12, "[3.13] gh-1: Fix it (GH-10)", vec![]),
                bots_backport(13, "[3.12] gh-1: Fix it (GH-99)"),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![11, 12]), (13, vec![])]);
        let kinds: Vec<_> = first_issue(&found)
            .pull_requests
            .iter()
            .flat_map(|entry| match entry {
                TreeEntry::Pr(tree) => tree.backports.clone(),
                TreeEntry::Backport(backport) => vec![backport.clone()],
            })
            .map(|row| row.backport)
            .collect();
        assert_eq!(
            kinds,
            [BackportKind::Bot, BackportKind::Manual, BackportKind::Bot]
        );
    }

    #[test]
    fn a_backport_under_a_cherry_pick_line_finds_its_origin_pr() {
        let sha = CommitSha::parse("0123abc").unwrap();
        let origin = PullRequestFacts {
            state: PrState::Merged,
            merge_commit: Some(sha.clone()),
            ..fixing(10, 1)
        };
        let by_hand = PullRequestFacts {
            off_default: Some(OffDefault {
                description: String::new(),
                cherry_picked_from: vec![sha],
                committers: vec![],
            }),
            ..own(backport(11, "gh-1: Fix it on 3.14", vec![]))
        };
        let found = workbench(vec![origin, by_hand], vec![issue(1)]);
        assert_eq!(tree_of(&found), [(10, vec![11])]);
    }

    #[test]
    fn a_backport_of_another_issues_pr_brings_that_pr_under_its_own_issue() {
        let origin = fixing(10, 1);
        let found = workbench(
            vec![origin, backport(11, "[3.14] gh-2: Fix it (GH-10)", vec![])],
            vec![issue(1), issue(2)],
        );
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10, 11]), ("issue", 1, vec![10])]
        );
    }

    #[test]
    fn a_backport_that_fixes_no_issue_follows_its_origin_pr() {
        let both = PullRequestFacts {
            closing_issues: vec![2],
            ..fixing(10, 1)
        };
        let found = workbench(
            vec![both, backport(11, "[3.14] Fix it (GH-10)", vec![])],
            vec![issue(1), issue(2)],
        );
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10, 11]), ("issue", 1, vec![10, 11])]
        );
    }

    #[test]
    fn a_backport_whose_origin_pr_is_missing_stands_in_the_issue_by_itself() {
        let found = workbench(
            vec![
                fixing(10, 1),
                backport(11, "[3.14] gh-1: Fix it (GH-99)", vec![]),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![]), (11, vec![])]);
    }

    #[test]
    fn a_backport_of_a_backport_stands_by_itself_so_a_tree_stays_two_levels() {
        let found = workbench(
            vec![
                fixing(10, 1),
                backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![]),
                backport(12, "[3.13] gh-1: Fix it (GH-11)", vec![]),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![11]), (12, vec![])]);
    }

    #[test]
    fn a_pr_with_no_issue_keeps_its_backports_beneath_it() {
        let found = workbench(
            vec![mine(10), backport(11, "[3.14] Fix it (GH-10)", vec![])],
            vec![],
        );
        assert_eq!(outline(&found), [("standalone", 10, vec![11])]);
    }

    #[test]
    fn a_users_backport_lists_an_origin_pr_that_is_not_the_users() {
        let found = workbench(
            vec![
                PullRequestFacts {
                    number: 10,
                    title: "Fix it".to_owned(),
                    ..stranger()
                },
                own(backport(11, "[3.14] Fix it (GH-10)", vec![])),
            ],
            vec![],
        );
        assert_eq!(outline(&found), [("standalone", 10, vec![11])]);
    }

    #[test]
    fn a_users_backport_with_no_origin_pr_and_no_issue_stands_by_itself() {
        let found = workbench(
            vec![own(backport(11, "[3.14] Fix it (GH-10)", vec![]))],
            vec![],
        );
        assert_eq!(outline(&found), [("standalone backport", 11, vec![])]);
    }

    #[test]
    fn a_backport_standing_by_itself_takes_its_place_by_its_time() {
        let later = PullRequestFacts {
            updated_at: at(9),
            ..own(backport(11, "[3.14] Fix it (GH-10)", vec![]))
        };
        let found = workbench(vec![mine(20), later], vec![]);
        assert_eq!(
            outline(&found),
            [
                ("standalone backport", 11, vec![]),
                ("standalone", 20, vec![])
            ]
        );
    }

    #[test]
    fn someone_elses_backport_with_no_origin_pr_and_no_issue_isnt_listed() {
        let found = workbench(vec![backport(11, "[3.14] Fix it (GH-10)", vec![])], vec![]);
        assert_eq!(outline(&found), []);
    }

    #[test]
    fn a_review_request_on_a_backport_lists_its_issue() {
        let requested = PullRequestFacts {
            review_requests: vec![me()],
            ..backport(11, "[3.14] gh-1: Fix it (GH-99)", vec![])
        };
        let found = workbench(vec![requested], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![11])]);
    }

    #[test]
    fn a_tree_holds_merged_and_closed_prs_and_says_which_they_are() {
        let in_state = |number, state| PullRequestFacts {
            number,
            state,
            title: "gh-1: Fix it".to_owned(),
            ..stranger()
        };
        let found = workbench(
            vec![
                fixing(10, 1),
                in_state(11, PrState::Merged),
                in_state(12, PrState::Closed),
            ],
            vec![issue(1)],
        );
        let states: Vec<_> = first_issue(&found)
            .pull_requests
            .iter()
            .flat_map(TreeEntry::rows)
            .map(|row| row.state)
            .collect();
        assert_eq!(states, [PrState::Open, PrState::Merged, PrState::Closed]);
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
    fn an_open_backport_keeps_its_merged_origin_prs_issue_on_the_workbench() {
        let merged = PullRequestFacts {
            state: PrState::Merged,
            ..fixing(10, 1)
        };
        let found = workbench(
            vec![
                merged,
                own(backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![])),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![11])]);
    }

    #[test]
    fn an_issues_people_and_time_count_its_backports() {
        let later = PullRequestFacts {
            updated_at: at(7),
            author: Some(Login::new("some-backporter")),
            ..backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![])
        };
        let found = workbench(
            vec![
                fixing(10, 1),
                later,
                bots_backport(12, "[3.13] gh-1: Fix it (GH-10)"),
            ],
            vec![issue(1)],
        );
        let item = first_issue(&found);
        assert_eq!(item.updated_at, at(7));
        assert_eq!(
            item.people,
            [
                Login::new("issue-author"),
                me(),
                Login::new("some-backporter")
            ]
        );
    }

    #[test]
    fn under_githubs_conventions_a_pr_on_a_branch_is_an_ordinary_pr() {
        let github = RepositorySettings {
            conventions: Conventions::Github,
            ..cpython()
        };
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![mine(10), own(backport(11, "[3.14] Fix it (GH-10)", vec![]))],
            issues: vec![],
            filters: BTreeMap::new(),
        };
        let found = Workbench::from_facts(facts, &github, None);
        assert_eq!(
            outline(&found),
            [("standalone", 11, vec![]), ("standalone", 10, vec![])]
        );
    }

    #[test]
    fn nothing_is_the_users_before_anyone_signs_in() {
        let found = Workbench::from_facts(
            WorkFacts {
                viewer: None,
                pull_requests: vec![mine(1)],
                issues: vec![issue(1)],
                filters: BTreeMap::new(),
            },
            &cpython(),
            None,
        );
        assert_eq!(found.items, []);
    }
}
