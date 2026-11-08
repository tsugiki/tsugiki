//! The Filters' part of a Workbench (ADR 0015): the Sidebar's lines, and which items each search reaches.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use super::{ListedItem, WorkbenchItem, count_of};
use crate::filters::{Filter, FilterName, FilterQuery};
use crate::repository::RepositorySettings;
use crate::store::FilterResults;

/// A Filter's line of the Sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FilterLine {
    pub name: FilterName,
    pub query: FilterQuery,
    /// The count of its items that Need attention.
    pub count: u32,
    /// The words for the failure that stopped its last search from being whole.
    pub problem: Option<String>,
}

/// A Repository's Filters, with what each last found.
pub(super) struct FilterFacts<'a> {
    filters: &'a [Filter],
    /// Only the searches of `filters`: a search the user has edited away reaches nothing.
    results: BTreeMap<FilterQuery, FilterResults>,
    nothing: FilterResults,
}

impl<'a> FilterFacts<'a> {
    pub(super) fn new(
        repository: &'a RepositorySettings,
        mut results: BTreeMap<FilterQuery, FilterResults>,
    ) -> Self {
        let filters = repository.filters.as_slice();
        results.retain(|query, _| filters.iter().any(|filter| filter.query == *query));
        Self {
            filters,
            results,
            nothing: FilterResults::default(),
        }
    }

    /// The items any Filter found.
    pub(super) fn matched(&self) -> BTreeSet<u32> {
        let found = self.results.values();
        found
            .flat_map(|results| results.numbers.iter().copied())
            .collect()
    }

    pub(super) fn numbers_of(&self, query: &FilterQuery) -> &BTreeSet<u32> {
        &self.results_of(query).numbers
    }

    /// The Sidebar's line for each Filter, in the order the settings hold them.
    pub(super) fn lines(&self, candidates: &[ListedItem]) -> Vec<FilterLine> {
        let line = |filter: &Filter| {
            let results = self.results_of(&filter.query);
            let reached = candidates
                .iter()
                .filter(|listed| listed.attention.is_some())
                .filter(|listed| listed.item.is_reached_by(&results.numbers));
            FilterLine {
                name: filter.name.clone(),
                query: filter.query.clone(),
                count: count_of(reached),
                problem: results
                    .problem
                    .as_ref()
                    .map(|problem| problem.words().to_owned()),
            }
        };
        self.filters.iter().map(line).collect()
    }

    fn results_of(&self, query: &FilterQuery) -> &FilterResults {
        self.results.get(query).unwrap_or(&self.nothing)
    }
}

impl WorkbenchItem {
    /// True when a search found this item: the Issue itself, or any PR in its tree (a Matched PR).
    pub(super) fn is_reached_by(&self, numbers: &BTreeSet<u32>) -> bool {
        let issue = match self {
            Self::Issue(issue) => numbers.contains(&issue.number),
            Self::StandalonePr(_) | Self::StandaloneBackport(_) => false,
        };
        issue || self.rows().iter().any(|row| numbers.contains(&row.number))
    }
}

#[cfg(test)]
mod tests;
