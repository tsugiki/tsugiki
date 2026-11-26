//! GitHub's answer to a search typed in the Inbox's box (#465): the results it listed, in its order, how many
//! the search reaches, and whether it left more than Tsugiki read.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

/// The most results Tsugiki reads of one search: GitHub lists 1,000, and Tsugiki asks for them ten pages of
/// a hundred at a time (#465).
pub const MAX_RESULTS: usize = 1000;

/// Whether a result GitHub found is an Issue or a PR. GitHub numbers both from one sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Issue,
    PullRequest,
}

/// One result of a typed search, as GitHub listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub number: u32,
    pub kind: RowKind,
    pub updated_at: OffsetDateTime,
}

/// GitHub's answer to a typed search (#465).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxSearch {
    /// The results GitHub listed, in its order.
    pub hits: Vec<SearchHit>,
    /// How many results the search reaches, even past the 1,000 GitHub lists.
    pub total: u32,
    /// Whether more results waited after the last page Tsugiki read, a search past 1,000 (#465).
    pub cut_off: bool,
}
