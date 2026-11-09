//! Each Filter's last results (ADR 0015). Tsugiki stores the items themselves as facts, so these rows say
//! which numbers belong to which search. A search the user has edited starts with nothing, and a sync removes
//! what's left of the old one. A refresh asks GitHub only about what changed, and merges the answer.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Row, named_params};

use super::Connection;
use crate::filters::FilterQuery;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE filter_matches (
        repository TEXT NOT NULL COLLATE NOCASE,
        query TEXT NOT NULL,
        number INTEGER NOT NULL,
        PRIMARY KEY (repository, query, number)
    ) WITHOUT ROWID;

    CREATE TABLE filter_problems (
        repository TEXT NOT NULL COLLATE NOCASE,
        query TEXT NOT NULL,
        problem TEXT NOT NULL,
        PRIMARY KEY (repository, query)
    ) WITHOUT ROWID;
";

/// The searches that have finished at least once, so a search that found nothing has a row too, and a refresh
/// can tell it from one that hasn't run yet.
pub(super) const SEARCHES: &str = "
    CREATE TABLE filter_searches (
        repository TEXT NOT NULL COLLATE NOCASE,
        query TEXT NOT NULL,
        cut_off INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (repository, query)
    ) WITHOUT ROWID;
";

const CUT_OFF: &str = "GitHub lists at most 1,000 results for one search, and over 1,000 items here were \
                       created on one day, so this Filter may be missing some.";

/// What stopped a Filter's last search from being whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterProblem {
    /// More results than GitHub lists fell on one day, so the search read as far as GitHub goes.
    CutOff,
    /// GitHub refused the search, in these words.
    Refused(String),
}

impl FilterProblem {
    /// The Sidebar's words for this.
    pub fn words(&self) -> &str {
        match self {
            Self::CutOff => CUT_OFF,
            Self::Refused(words) => words,
        }
    }
}

/// A Filter's results as the last sync left them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterResults {
    /// The Issues and PRs it found, which share GitHub's one list of numbers.
    pub numbers: BTreeSet<u32>,
    pub problem: Option<FilterProblem>,
}

/// What a search narrowed to recent updates says about a Filter (ADR 0015).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NarrowedResults {
    /// Each item updated in that time, whatever the Filter thinks of it.
    pub changed: BTreeSet<u32>,
    /// The items the search found. A changed item outside this has stopped matching, unless `cut_off` says
    /// the search may have missed it.
    pub found: BTreeSet<u32>,
    /// GitHub wouldn't list all of them. Without this, a problem stays as it is.
    pub cut_off: bool,
}

/// Replaces what `query` found, in one transaction (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
    results: &FilterResults,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    forget(&transaction, repository, query)?;
    mark_searched(&transaction, repository, query)?;
    for number in &results.numbers {
        add_match(&transaction, repository, query, *number)?;
    }
    match &results.problem {
        Some(FilterProblem::CutOff) => mark_cut_off(&transaction, repository, query)?,
        Some(FilterProblem::Refused(words)) => {
            save_problem(&transaction, repository, query, words)?
        }
        None => {}
    }
    transaction.commit()
}

/// Applies what a narrowed search said to what `query` found, in one transaction (ADR 0010). Answers with the
/// numbers that joined or left the Filter.
pub(super) fn merge(
    connection: &mut Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
    narrowed: &NarrowedResults,
) -> rusqlite::Result<BTreeSet<u32>> {
    let transaction = connection.transaction()?;
    let before = read_all(&transaction, repository)?
        .remove(query)
        .unwrap_or_default()
        .numbers;
    let joined: Vec<u32> = narrowed.found.difference(&before).copied().collect();
    let left: Vec<u32> = before
        .intersection(&narrowed.changed)
        .filter(|number| !narrowed.cut_off && !narrowed.found.contains(number))
        .copied()
        .collect();
    mark_searched(&transaction, repository, query)?;
    for number in &joined {
        add_match(&transaction, repository, query, *number)?;
    }
    for number in &left {
        let params =
            named_params! { ":repository": repository, ":query": query, ":number": number };
        let _removed = transaction.execute(
            "DELETE FROM filter_matches
             WHERE repository = :repository AND query = :query AND number = :number",
            params,
        )?;
    }
    if narrowed.cut_off {
        mark_cut_off(&transaction, repository, query)?;
    }
    transaction.commit()?;
    Ok(joined.into_iter().chain(left).collect())
}

fn mark_searched(
    connection: &Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO filter_searches (repository, query) VALUES (:repository, :query)",
            named_params! { ":repository": repository, ":query": query },
        )
        .map(drop)
}

fn mark_cut_off(
    connection: &Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "UPDATE filter_searches SET cut_off = 1 WHERE repository = :repository AND query = :query",
            named_params! { ":repository": repository, ":query": query },
        )
        .map(drop)
}

fn add_match(
    connection: &Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO filter_matches (repository, query, number)
             VALUES (:repository, :query, :number)",
            named_params! { ":repository": repository, ":query": query, ":number": number },
        )
        .map(drop)
}

/// Says what stopped a search, and leaves what it last found.
pub(super) fn save_problem(
    connection: &Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
    problem: &str,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO filter_problems (repository, query, problem)
             VALUES (:repository, :query, :problem)
             ON CONFLICT (repository, query) DO UPDATE SET problem = excluded.problem",
            named_params! { ":repository": repository, ":query": query, ":problem": problem },
        )
        .map(drop)
}

fn forget(
    connection: &Connection,
    repository: &RepositoryName,
    query: &FilterQuery,
) -> rusqlite::Result<()> {
    for sql in [
        "DELETE FROM filter_matches WHERE repository = :repository AND query = :query",
        "DELETE FROM filter_problems WHERE repository = :repository AND query = :query",
        "DELETE FROM filter_searches WHERE repository = :repository AND query = :query",
    ] {
        let params = named_params! { ":repository": repository, ":query": query };
        let _removed = connection.execute(sql, params)?;
    }
    Ok(())
}

/// Removes the results of each search `keep` doesn't hold, in one transaction.
pub(super) fn keep_only(
    connection: &mut Connection,
    repository: &RepositoryName,
    keep: &[FilterQuery],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let stored: BTreeSet<FilterQuery> = read_all(&transaction, repository)?.into_keys().collect();
    for query in stored.iter().filter(|query| !keep.contains(query)) {
        forget(&transaction, repository, query)?;
    }
    transaction.commit()
}

/// Each search of `repository`, with its results.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<FilterQuery, FilterResults>> {
    let mut found: BTreeMap<FilterQuery, FilterResults> = BTreeMap::new();
    let mut searches = connection
        .prepare("SELECT query, cut_off FROM filter_searches WHERE repository = :repository")?;
    let rows = searches.query_map(named_params! { ":repository": repository }, search_in)?;
    for row in rows {
        let (query, cut_off) = row?;
        found.entry(query).or_default().problem = cut_off.then_some(FilterProblem::CutOff);
    }
    let mut matches = connection
        .prepare("SELECT query, number FROM filter_matches WHERE repository = :repository")?;
    let rows = matches.query_map(named_params! { ":repository": repository }, match_in)?;
    for row in rows {
        let (query, number) = row?;
        let _new = found.entry(query).or_default().numbers.insert(number);
    }
    let mut problems = connection
        .prepare("SELECT query, problem FROM filter_problems WHERE repository = :repository")?;
    let rows = problems.query_map(named_params! { ":repository": repository }, problem_in)?;
    for row in rows {
        let (query, problem) = row?;
        found.entry(query).or_default().problem = Some(FilterProblem::Refused(problem));
    }
    Ok(found)
}

fn search_in(row: &Row<'_>) -> rusqlite::Result<(FilterQuery, bool)> {
    Ok((row.get("query")?, row.get("cut_off")?))
}

fn match_in(row: &Row<'_>) -> rusqlite::Result<(FilterQuery, u32)> {
    Ok((row.get("query")?, row.get("number")?))
}

fn problem_in(row: &Row<'_>) -> rusqlite::Result<(FilterQuery, String)> {
    Ok((row.get("query")?, row.get("problem")?))
}

#[cfg(test)]
mod tests;
