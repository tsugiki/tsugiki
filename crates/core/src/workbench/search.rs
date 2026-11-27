//! A typed search, read into the forms the cache matches (#464), and the rows it narrows a list to: the
//! Inbox's search box and the person Filter a clicked name opens (#472).
//!
//! The cache understands five forms and nothing more: bare words, matched against the title; `#123`;
//! `author:<login>`; `label:<name>`; and `is:issue` or `is:pr`. Tsugiki adds `is:open` unless the query names
//! a state: `is:open`, `is:closed`, `is:merged`, `is:unmerged` or `state:`. A query with anything outside
//! those forms is left to GitHub (#465), and lists no local rows until its answer comes.

use std::collections::{BTreeMap, BTreeSet};

use crate::facts::WorkFacts;
use crate::facts::inbox_search::RowKind;
use crate::facts::issues::IssueFacts;
use crate::facts::labels::Label;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::filters::FilterQuery;
use crate::login::Login;

use super::{TreeEntry, WorkbenchItem};

/// The text from a search box, read for the cache.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Search {
    /// The box's text without the spaces around it, which names GitHub's answer to it (#465).
    text: String,
    /// The forms a row has to meet, all of them.
    matches: Vec<Match>,
    /// Whether the text holds a form the cache doesn't read, so only GitHub can answer it.
    needs_github: bool,
}

/// One form of a search.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Match {
    /// A word the title has to hold, lowercased.
    Word(String),
    /// The number the whole item holds.
    Number(u32),
    /// The login of the row's author.
    Author(Login),
    /// A label the row carries, which ignores case as GitHub's do.
    Label(Label),
    /// Whether the row is an Issue or a PR.
    Kind(RowKind),
    /// Whether the row is open, or closed and merged.
    State(State),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Open,
    Closed,
}

/// Whether a list counts an Issue open. A typed search and the Inbox disagree: the Inbox lists a closed
/// Issue a PR brought back in, while a typed search reads the Issue's own state (#472, #823).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IssueOpen {
    Open,
    Closed,
}

/// One Issue's or PR's title, author, labels and state, as a search reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchRow {
    pub(super) kind: RowKind,
    pub(super) number: u32,
    /// The title, lowercased: a bare word matches whether it was typed in either case.
    title: String,
    author: Option<Login>,
    labels: Vec<Label>,
    open: bool,
}

impl SearchRow {
    /// Issue `facts` as a search reads it, `open` saying whether the list it narrows counts it open.
    pub(super) fn issue(facts: &IssueFacts, open: IssueOpen) -> Self {
        Self {
            kind: RowKind::Issue,
            number: facts.number,
            title: facts.title.to_lowercase(),
            author: facts.author.clone(),
            labels: facts.labels.clone(),
            open: open == IssueOpen::Open,
        }
    }

    pub(super) fn pull_request(facts: &PullRequestFacts) -> Self {
        Self {
            kind: RowKind::PullRequest,
            number: facts.number,
            title: facts.title.to_lowercase(),
            author: facts.author.clone(),
            labels: facts.labels.clone(),
            open: facts.state == PrState::Open,
        }
    }
}

impl Search {
    /// Reads `text` into the forms the cache matches.
    pub(super) fn parse(text: &str) -> Self {
        let mut search = Self {
            text: text.trim().to_owned(),
            matches: Vec::new(),
            needs_github: false,
        };
        // A quoted phrase, or a word the user excludes, is GitHub's to read.
        if text.contains('"') {
            search.needs_github = true;
        }
        let mut names_state = false;
        for token in text.split_whitespace() {
            if token.starts_with('-') {
                search.needs_github = true;
                continue;
            }
            names_state |= crate::filters::names_a_state(token);
            let Some((qualifier, value)) = token.split_once(':') else {
                match token.strip_prefix('#') {
                    Some(digits) => match digits.parse() {
                        Ok(number) => search.matches.push(Match::Number(number)),
                        Err(_) => search.needs_github = true,
                    },
                    None => search.matches.push(Match::Word(token.to_lowercase())),
                }
                continue;
            };
            let qualifier = qualifier.to_ascii_lowercase();
            match (qualifier.as_str(), value) {
                ("author", value) if !value.is_empty() && !value.starts_with('@') => {
                    search.matches.push(Match::Author(Login::new(value)));
                }
                ("label", value) if !value.is_empty() => {
                    search.matches.push(Match::Label(Label::new(value)));
                }
                ("is", value) => match value.to_ascii_lowercase().as_str() {
                    "issue" => search.matches.push(Match::Kind(RowKind::Issue)),
                    "pr" => search.matches.push(Match::Kind(RowKind::PullRequest)),
                    "open" => search.matches.push(Match::State(State::Open)),
                    "closed" => search.matches.push(Match::State(State::Closed)),
                    // The states GitHub knows and the cache doesn't: they keep Tsugiki from adding
                    // `is:open`, and the answer waits on GitHub.
                    "merged" | "unmerged" => search.needs_github = true,
                    _ => search.needs_github = true,
                },
                ("state", _) => search.needs_github = true,
                // Anything else names a form the cache doesn't read, or holds no value at all.
                _ => search.needs_github = true,
            }
        }
        if !names_state && !text.trim().is_empty() {
            search.matches.push(Match::State(State::Open));
        }
        search
    }

    /// The box's text, without the spaces around it: what GitHub is asked, and what keys its answer (#465).
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Whether the box held nothing, so the list reads its pages as it does without a search.
    pub(super) fn is_empty(&self) -> bool {
        self.matches.is_empty() && !self.needs_github
    }

    /// Whether the text holds a form only GitHub reads, so the list has no local rows and says so.
    pub(super) fn needs_github(&self) -> bool {
        self.needs_github
    }

    /// The Filter query this search saves as: its text with Tsugiki's additions spelled out (#471).
    pub(super) fn filter_query(&self) -> Option<FilterQuery> {
        FilterQuery::with_additions(&self.text)
    }

    /// Whether `row` meets each form of the search. A text the cache can't read matches nothing.
    pub(super) fn finds(&self, row: &SearchRow) -> bool {
        !self.needs_github && self.matches.iter().all(|one| one.finds(row))
    }
}

impl Match {
    fn finds(&self, row: &SearchRow) -> bool {
        match self {
            Self::Word(word) => row.title.contains(word),
            Self::Number(number) => row.number == *number,
            Self::Author(login) => row.author.as_ref() == Some(login),
            Self::Label(label) => row.labels.contains(label),
            Self::Kind(kind) => row.kind == *kind,
            Self::State(State::Open) => row.open,
            Self::State(State::Closed) => !row.open,
        }
    }
}

/// The cached Issues and PRs a search reads, by number (#464).
pub(super) struct Index {
    issues: BTreeMap<u32, SearchRow>,
    pull_requests: BTreeMap<u32, SearchRow>,
}

impl Index {
    /// The cache `facts` holds, with `open` saying which Issues count open, as `SearchRow::issue` reads them.
    pub(super) fn of(facts: &WorkFacts, open: impl Fn(&IssueFacts) -> IssueOpen) -> Self {
        Self {
            issues: (facts.issues.iter())
                .map(|issue| (issue.number, SearchRow::issue(issue, open(issue))))
                .collect(),
            pull_requests: (facts.pull_requests.iter())
                .map(|pr| (pr.number, SearchRow::pull_request(pr)))
                .collect(),
        }
    }

    /// The row of the PR `number`, which the list draws under an Issue or on its own.
    fn pull_request(&self, number: u32) -> Option<&SearchRow> {
        self.pull_requests.get(&number)
    }

    /// Whether the cache holds Issue or PR `number`.
    pub(super) fn holds(&self, number: u32) -> bool {
        self.issues.contains_key(&number) || self.pull_requests.contains_key(&number)
    }

    /// Each Issue and PR number in the cache that `search` finds.
    pub(super) fn matching(&self, search: &Search) -> BTreeSet<u32> {
        let issues = (self.issues.iter())
            .filter(|(_, row)| search.finds(row))
            .map(|(number, _)| *number);
        let pull_requests = (self.pull_requests.iter())
            .filter(|(_, row)| search.finds(row))
            .map(|(number, _)| *number);
        issues.chain(pull_requests).collect()
    }
}

/// `item` with only the rows `search` finds in the cache, or nothing when it finds none.
pub(super) fn narrowed(
    item: WorkbenchItem,
    search: &Search,
    rows: &Index,
) -> Option<WorkbenchItem> {
    narrowed_by(item, &|kind, number| match kind {
        RowKind::Issue => (rows.issues.get(&number)).is_some_and(|row| search.finds(row)),
        RowKind::PullRequest => rows
            .pull_request(number)
            .is_some_and(|row| search.finds(row)),
    })
}

/// GitHub's place for the first result `item` holds, or nothing when it holds no result GitHub returned.
pub(super) fn rank_of(item: &WorkbenchItem, positions: &BTreeMap<u32, usize>) -> Option<usize> {
    (item.keys().into_iter())
        .filter_map(|key| positions.get(&key.number()).copied())
        .min()
}

/// `item` with only the rows `finds` keeps, or nothing when it keeps none. An Issue's header, and a PR's
/// over the Backports beneath it, stays over the rows that match beneath it, even when it doesn't match
/// itself, so a matching PR stays in its Issue's fold (#464).
pub(super) fn narrowed_by(
    item: WorkbenchItem,
    finds: &dyn Fn(RowKind, u32) -> bool,
) -> Option<WorkbenchItem> {
    match item {
        WorkbenchItem::Issue(mut issue) => {
            let header = finds(RowKind::Issue, issue.number);
            issue.pull_requests = (issue.pull_requests.into_iter())
                .filter_map(|entry| match entry {
                    TreeEntry::Pr(mut tree) => {
                        let top = finds(RowKind::PullRequest, tree.row.number);
                        tree.backports
                            .retain(|backport| finds(RowKind::PullRequest, backport.row.number));
                        (top || !tree.backports.is_empty()).then_some(TreeEntry::Pr(tree))
                    }
                    TreeEntry::Backport(backport) => {
                        finds(RowKind::PullRequest, backport.row.number)
                            .then_some(TreeEntry::Backport(backport))
                    }
                })
                .collect();
            (header || !issue.pull_requests.is_empty()).then_some(WorkbenchItem::Issue(issue))
        }
        WorkbenchItem::StandalonePr(mut tree) => {
            let top = finds(RowKind::PullRequest, tree.row.number);
            tree.backports
                .retain(|backport| finds(RowKind::PullRequest, backport.row.number));
            (top || !tree.backports.is_empty()).then_some(WorkbenchItem::StandalonePr(tree))
        }
        WorkbenchItem::StandaloneBackport(backport) => {
            finds(RowKind::PullRequest, backport.row.number)
                .then_some(WorkbenchItem::StandaloneBackport(backport))
        }
        // A placeholder is already a result, with no rows to narrow (#465).
        WorkbenchItem::Placeholder(_) => Some(item),
    }
}

#[cfg(test)]
mod tests;
