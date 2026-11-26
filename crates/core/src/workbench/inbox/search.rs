//! What the Inbox's search box holds, read into the forms the cache matches (#464).
//!
//! The cache understands five forms and nothing more: bare words, matched against the title; `#123`;
//! `author:<login>`; `label:<name>`; and `is:issue` or `is:pr`. Tsugiki adds `is:open` unless the query names
//! a state: `is:open`, `is:closed`, `is:merged`, `is:unmerged` or `state:`. A query with anything outside
//! those forms is left to GitHub (#465), and lists no local rows until its answer comes.

use std::collections::BTreeSet;

use crate::facts::inbox_search::RowKind;
use crate::facts::issues::IssueFacts;
use crate::facts::labels::Label;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::login::Login;

/// The text from the Inbox's search box, read for the cache.
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
    pub(super) fn issue(facts: &IssueFacts, brought_back: &BTreeSet<u32>) -> Self {
        Self {
            kind: RowKind::Issue,
            number: facts.number,
            title: facts.title.to_lowercase(),
            author: facts.author.clone(),
            labels: facts.labels.clone(),
            // The Inbox lists an open Issue, or a closed one a PR brought back.
            open: super::lists_issue(facts.state, facts.number, brought_back),
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

    /// Whether the box held nothing, so the Inbox reads its pages as it does without a search.
    pub(super) fn is_empty(&self) -> bool {
        self.matches.is_empty() && !self.needs_github
    }

    /// Whether the text holds a form only GitHub reads, so the list has no local rows and says so.
    pub(super) fn needs_github(&self) -> bool {
        self.needs_github
    }

    /// Whether `row` meets every form of the search. A text the cache can't read meets none.
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

#[cfg(test)]
mod tests;
