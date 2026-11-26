//! The Filters' part of a Workbench (ADR 0015): the Sidebar's lines, and which items each search reaches.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use super::{ListedItem, WorkbenchItem, count_of};
use crate::facts::filters::FilterResults;
use crate::filters::{Filter, FilterName, FilterQuery, OnWorkbench};
use crate::repository_settings::RepositorySettings;
use crate::work::matched;

/// A Filter's line of the Sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FilterLine {
    pub name: FilterName,
    pub query: FilterQuery,
    /// Whether its items go on the Workbench (#819).
    pub on_workbench: OnWorkbench,
    /// On the Workbench, the count of its items that Need attention. Off it, the count of the items its
    /// search found, as a view the user browses has nothing of the Workbench's to wait on.
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

    /// The items the Filters on the Workbench found.
    pub(super) fn matched(&self) -> BTreeSet<u32> {
        matched(self.filters, &self.results)
    }

    /// The items any Filter found, on the Workbench or off it.
    pub(super) fn found(&self) -> BTreeSet<u32> {
        let numbers = self.results.values().flat_map(|results| &results.numbers);
        numbers.copied().collect()
    }

    pub(super) fn numbers_of(&self, query: &FilterQuery) -> &BTreeSet<u32> {
        &self.results_of(query).numbers
    }

    /// The Sidebar's line for each Filter, in the order the settings hold them. One on the Workbench counts
    /// the items of `on_workbench` that Need attention, and one off it the items of `unhidden`.
    pub(super) fn lines(
        &self,
        on_workbench: &[&ListedItem],
        unhidden: &[&ListedItem],
    ) -> Vec<FilterLine> {
        let line = |filter: &Filter| {
            let results = self.results_of(&filter.query);
            let candidates = match filter.on_workbench {
                OnWorkbench::On => on_workbench,
                OnWorkbench::Off => unhidden,
            };
            let reached = candidates
                .iter()
                .filter(|listed| match filter.on_workbench {
                    OnWorkbench::On => listed.attention.is_some(),
                    OnWorkbench::Off => true,
                })
                .filter(|listed| listed.item.is_reached_by(&results.numbers));
            FilterLine {
                name: filter.name.clone(),
                query: filter.query.clone(),
                on_workbench: filter.on_workbench,
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
            Self::StandalonePr(_) | Self::StandaloneBackport(_) | Self::Placeholder(_) => false,
        };
        issue || self.rows().iter().any(|row| numbers.contains(&row.number))
    }
}

#[cfg(test)]
mod tests;
