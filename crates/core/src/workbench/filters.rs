//! The Filters' part of a Workbench (ADR 0015): the Sidebar's lines, and which items each search reaches.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use super::{WorkbenchItem, count_of};
use crate::filters::{Filter, FilterName, FilterQuery};
use crate::repository::RepositorySettings;
use crate::store::FilterResults;

/// A Filter's line of the Sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FilterLine {
    pub name: FilterName,
    pub query: FilterQuery,
    /// How many items it holds, until Needs attention exists to count instead.
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
    pub(super) fn lines(&self, candidates: &[WorkbenchItem]) -> Vec<FilterLine> {
        let line = |filter: &Filter| {
            let results = self.results_of(&filter.query);
            let reached = candidates
                .iter()
                .filter(|item| item.is_reached_by(&results.numbers));
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
mod tests {
    use super::*;
    use crate::github::pull_requests::{PrState, PullRequestFacts};
    use crate::store::{FilterProblem, WorkFacts};
    use crate::workbench::Workbench;
    use crate::workbench::testing::{
        backport, cpython, fixing, issue, me, mine, outline, stranger,
    };

    fn query(text: &str) -> FilterQuery {
        text.to_owned().try_into().unwrap()
    }

    /// A Repository whose Filters are each named after their search.
    fn with_filters(queries: &[&str]) -> RepositorySettings {
        RepositorySettings {
            filters: queries
                .iter()
                .map(|text| Filter {
                    name: format!("Named {text}").try_into().unwrap(),
                    query: query(text),
                })
                .collect(),
            ..cpython()
        }
    }

    fn found_by(numbers: &[u32]) -> FilterResults {
        FilterResults {
            numbers: numbers.iter().copied().collect(),
            problem: None,
        }
    }

    /// PRs #10 by someone else, fixing Issue 1; #20 by someone else, fixing nothing; and Issue 2 with no PR.
    fn nobodys_work(filters: &[(&str, FilterResults)]) -> WorkFacts {
        WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![
                PullRequestFacts {
                    number: 10,
                    ..stranger()
                },
                PullRequestFacts {
                    number: 20,
                    title: "PR 20".to_owned(),
                    ..stranger()
                },
            ],
            issues: vec![issue(1), issue(2)],
            filters: filters
                .iter()
                .map(|(text, results)| (query(text), results.clone()))
                .collect(),
        }
    }

    #[test]
    fn nothing_a_filter_found_means_nothing_of_someone_elses_is_listed() {
        let found = Workbench::from_facts(nobodys_work(&[]), &with_filters(&["a"]), None);
        assert_eq!(found.items, []);
        assert_eq!(found.count, 0);
        assert_eq!(found.filters[0].count, 0);
    }

    #[test]
    fn the_workbench_lists_an_issue_a_filter_found_with_its_tree() {
        let facts = nobodys_work(&[("a", found_by(&[1]))]);
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("issue", 1, vec![10])]);
        assert_eq!(found.count, 1);
    }

    #[test]
    fn the_workbench_lists_an_issue_with_no_prs_that_a_filter_found() {
        let facts = nobodys_work(&[("a", found_by(&[2]))]);
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("issue", 2, vec![])]);
    }

    #[test]
    fn a_pr_a_filter_found_pulls_its_issue_onto_the_workbench() {
        let facts = nobodys_work(&[("a", found_by(&[10]))]);
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("issue", 1, vec![10])]);
    }

    #[test]
    fn a_pr_a_filter_found_that_fixes_no_issue_stands_by_itself() {
        let facts = nobodys_work(&[("a", found_by(&[20]))]);
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("standalone", 20, vec![])]);
    }

    #[test]
    fn a_closed_pr_a_filter_found_stays_with_its_issue() {
        let mut facts = nobodys_work(&[("a", found_by(&[10]))]);
        facts.pull_requests[0].state = PrState::Closed;
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("issue", 1, vec![10])]);
    }

    #[test]
    fn a_filter_found_backport_brings_its_whole_tree() {
        let mut facts = nobodys_work(&[("a", found_by(&[30]))]);
        facts
            .pull_requests
            .push(backport(30, "[3.14] Fix it (GH-20)", vec![]));
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("standalone", 20, vec![30])]);
    }

    #[test]
    fn a_lone_backport_a_filter_found_stands_by_itself() {
        let mut facts = nobodys_work(&[("a", found_by(&[30]))]);
        facts
            .pull_requests
            .push(backport(30, "[3.14] Fix it (GH-99)", vec![]));
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("standalone backport", 30, vec![])]);
    }

    #[test]
    fn a_matched_pr_comes_first_in_its_issues_tree() {
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![fixing(10, 1), fixing(11, 1), fixing(12, 1)],
            issues: vec![issue(1)],
            filters: BTreeMap::from([(query("a"), found_by(&[12]))]),
        };
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(outline(&found), [("issue", 1, vec![12, 10, 11])]);
    }

    #[test]
    fn a_selected_filters_own_pr_comes_first_in_its_issues_tree() {
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![fixing(10, 1), fixing(11, 1)],
            issues: vec![issue(1)],
            filters: BTreeMap::from([(query("a"), found_by(&[10])), (query("b"), found_by(&[11]))]),
        };
        let selected = query("b");
        let found = Workbench::from_facts(facts, &with_filters(&["a", "b"]), Some(&selected));
        assert_eq!(outline(&found), [("issue", 1, vec![11, 10])]);
    }

    #[test]
    fn selecting_a_filter_shows_only_what_it_found_even_if_its_not_the_users() {
        let facts = WorkFacts {
            pull_requests: vec![
                mine(40),
                PullRequestFacts {
                    number: 20,
                    title: "PR 20".to_owned(),
                    ..stranger()
                },
            ],
            ..nobodys_work(&[("a", found_by(&[1])), ("b", found_by(&[20]))])
        };
        let repository = with_filters(&["a", "b"]);

        let selected = query("b");
        let found = Workbench::from_facts(facts, &repository, Some(&selected));

        assert_eq!(outline(&found), [("standalone", 20, vec![])]);
    }

    #[test]
    fn a_selected_search_no_filter_holds_shows_nothing() {
        let facts = nobodys_work(&[("gone", found_by(&[1]))]);
        let selected = query("gone");
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), Some(&selected));
        assert_eq!(found.items, []);
    }

    #[test]
    fn the_sidebar_counts_each_filter_and_the_workbench_whatever_is_selected() {
        let facts = WorkFacts {
            pull_requests: vec![
                mine(40),
                PullRequestFacts {
                    number: 10,
                    ..stranger()
                },
            ],
            ..nobodys_work(&[("a", found_by(&[1, 2, 10])), ("b", found_by(&[2]))])
        };
        let selected = query("b");
        let found = Workbench::from_facts(facts, &with_filters(&["a", "b"]), Some(&selected));

        // Issue 1 once, however many of its rows the Filter found.
        let counts: Vec<u32> = found.filters.iter().map(|line| line.count).collect();
        assert_eq!(counts, [2, 1]);
        // Those two Issues, and the user's own PR.
        assert_eq!(found.count, 3);
        assert_eq!(outline(&found), [("issue", 2, vec![])]);
    }

    #[test]
    fn the_sidebar_lists_the_filters_as_the_settings_do_with_their_problems() {
        let cut_off = FilterResults {
            numbers: BTreeSet::new(),
            problem: Some(FilterProblem::Refused("It was cut off".to_owned())),
        };
        let facts = nobodys_work(&[("b", cut_off), ("gone", found_by(&[1]))]);
        let found = Workbench::from_facts(facts, &with_filters(&["b", "a"]), None);

        let lines: Vec<(String, String, Option<String>)> = found
            .filters
            .into_iter()
            .map(|line| (line.name.into(), line.query.into(), line.problem))
            .collect();
        assert_eq!(
            lines,
            [
                (
                    "Named b".to_owned(),
                    "b".to_owned(),
                    Some("It was cut off".to_owned())
                ),
                ("Named a".to_owned(), "a".to_owned(), None),
            ]
        );
        // The results of a search the user has edited away reach nothing.
        assert_eq!(found.items, []);
    }

    #[test]
    fn before_anyone_signs_in_the_sidebar_still_lists_the_filters() {
        let mut facts = nobodys_work(&[("a", found_by(&[1]))]);
        facts.viewer = None;
        let found = Workbench::from_facts(facts, &with_filters(&["a"]), None);
        assert_eq!(found.filters.len(), 1);
        assert_eq!(found.filters[0].count, 0);
    }
}
