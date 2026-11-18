//! The store's facts of the PRs' Conversations, from which the rules work out those worth reading ahead.

use std::collections::{BTreeMap, BTreeSet};

use super::CommitSha;

/// A Repository's Conversations as the store holds them, read at one moment.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReadAheadFacts {
    /// The PRs whose Conversations the store owes a read.
    pub owed: BTreeSet<u32>,
    /// The head each held Conversation was read at, of those read with one.
    pub read_at: BTreeMap<u32, CommitSha>,
}
