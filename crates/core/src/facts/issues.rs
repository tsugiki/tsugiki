//! What Tsugiki keeps about an Issue a PR names, and about a PR that mentions an Issue.

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::facts::labels::Label;
use crate::facts::pull_requests::NodeId;
use crate::login::Login;

/// An Issue a PR names, as Tsugiki keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFacts {
    pub number: u32,
    pub title: String,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    pub assignees: Vec<Login>,
    /// In name order, ignoring case. Empty for an Issue stored before Tsugiki kept labels, until GitHub
    /// sends it again.
    pub labels: Vec<Label>,
    pub updated_at: OffsetDateTime,
    /// Nothing for an Issue stored before Tsugiki kept states, until GitHub sends it again.
    pub state: Option<IssueState>,
}

/// What places an Issue in a list, and nothing else, so a read of many Issues stays cheap. It carries none
/// of what Needs attention reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssuePlace {
    pub number: u32,
    pub updated_at: OffsetDateTime,
    /// Nothing for an Issue stored before Tsugiki kept states, until GitHub sends it again.
    pub state: Option<IssueState>,
}

/// Whether an Issue is open, and how it closed, as GitHub draws it (#188).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueState {
    Open,
    /// Closed as completed, or closed before GitHub said why.
    Closed,
    NotPlanned,
    /// Closed as a duplicate, which GitHub draws as it draws not planned.
    Duplicate,
}

/// A PR that mentions an Issue. Only the link rules can say whether it belongs in the Issue's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedPr {
    pub id: NodeId,
    pub number: u32,
    #[serde(rename = "updatedAt", with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
