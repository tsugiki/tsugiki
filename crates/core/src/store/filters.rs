//! Each Filter's last results (ADR 0015). Tsugiki stores the items themselves as facts, so these rows say
//! which numbers belong to which search. A search the user has edited starts with nothing, and a sync removes
//! what's left of the old one.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Row, named_params};

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

/// A Filter's results as the last sync left them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterResults {
    /// The Issues and PRs it found, which share GitHub's one list of numbers.
    pub numbers: BTreeSet<u32>,
    /// The Sidebar's words for the failure that stopped the last search from being whole.
    pub problem: Option<String>,
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
    for number in &results.numbers {
        let _inserted = transaction.execute(
            "INSERT INTO filter_matches (repository, query, number)
             VALUES (:repository, :query, :number)",
            named_params! { ":repository": repository, ":query": query, ":number": number },
        )?;
    }
    if let Some(problem) = &results.problem {
        save_problem(&transaction, repository, query, problem)?;
    }
    transaction.commit()
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
        found.entry(query).or_default().problem = Some(problem);
    }
    Ok(found)
}

fn match_in(row: &Row<'_>) -> rusqlite::Result<(FilterQuery, u32)> {
    Ok((row.get("query")?, row.get("number")?))
}

fn problem_in(row: &Row<'_>) -> rusqlite::Result<(FilterQuery, String)> {
    Ok((row.get("query")?, row.get("problem")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::store::testing::temp_database;
    use crate::store::{OpenReader, Store, StoreReader, reader};
    use std::fs;
    use std::path::PathBuf;

    /// A store and a reader of the same file.
    fn opened(name: &str) -> (Store, OpenReader, StoreReader, PathBuf, PathBuf) {
        let (path, folder) = temp_database(name);
        let store = Store::open(&path).unwrap();
        let (to_open, reader) = reader();
        let open = to_open.open(&path).unwrap();
        (store, open, reader, path, folder)
    }

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn query(text: &str) -> FilterQuery {
        text.to_owned().try_into().unwrap()
    }

    fn found(numbers: &[u32]) -> FilterResults {
        FilterResults {
            numbers: numbers.iter().copied().collect(),
            problem: None,
        }
    }

    #[test]
    fn a_new_search_replaces_what_the_last_one_found() {
        let (store, open, reader, _path, folder) = opened("filter-replace");
        store
            .save_filter_results(&cpython(), &query("a"), found(&[1, 2]))
            .unwrap();
        store
            .save_filter_results(&cpython(), &query("a"), found(&[2, 3]))
            .unwrap();
        store
            .save_filter_results(&cpython(), &query("b"), found(&[9]))
            .unwrap();

        let read = reader.work_facts(&cpython()).unwrap().filters;
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read[&query("a")], found(&[2, 3]));
        assert_eq!(read[&query("b")], found(&[9]));
    }

    #[test]
    fn each_repository_has_its_own_results_whatever_the_case_of_its_name() {
        let (store, open, reader, _path, folder) = opened("filter-repositories");
        let other = RepositoryName::parse("python/peps").unwrap();
        let shouted = RepositoryName::parse("Python/CPython").unwrap();
        store
            .save_filter_results(&cpython(), &query("a"), found(&[1]))
            .unwrap();
        store
            .save_filter_results(&other, &query("a"), found(&[2]))
            .unwrap();

        let read = reader.work_facts(&shouted).unwrap().filters;
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read[&query("a")], found(&[1]));
    }

    #[test]
    fn a_problem_leaves_what_the_last_search_found_until_a_search_succeeds() {
        let (store, open, reader, _path, folder) = opened("filter-problem");
        store
            .save_filter_results(&cpython(), &query("a"), found(&[1]))
            .unwrap();
        store
            .save_filter_problem(&cpython(), &query("a"), "It failed")
            .unwrap();
        store
            .save_filter_problem(&cpython(), &query("a"), "It failed again")
            .unwrap();
        let during = reader.work_facts(&cpython()).unwrap().filters;
        store
            .save_filter_results(&cpython(), &query("a"), found(&[2]))
            .unwrap();
        let after = reader.work_facts(&cpython()).unwrap().filters;
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(
            during[&query("a")],
            FilterResults {
                numbers: BTreeSet::from([1]),
                problem: Some("It failed again".to_owned()),
            }
        );
        assert_eq!(after[&query("a")], found(&[2]));
    }

    #[test]
    fn results_can_come_with_a_problem() {
        let (store, open, reader, _path, folder) = opened("filter-cut-off");
        let cut_off = FilterResults {
            numbers: BTreeSet::from([1]),
            problem: Some("It was cut off".to_owned()),
        };
        store
            .save_filter_results(&cpython(), &query("a"), cut_off.clone())
            .unwrap();

        let read = reader.work_facts(&cpython()).unwrap().filters;
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read[&query("a")], cut_off);
    }

    #[test]
    fn a_search_no_filter_holds_any_more_goes_with_its_problem() {
        let (store, open, reader, _path, folder) = opened("filter-edited");
        store
            .save_filter_results(&cpython(), &query("old"), found(&[1]))
            .unwrap();
        store
            .save_filter_problem(&cpython(), &query("broken"), "It failed")
            .unwrap();
        store
            .save_filter_results(&cpython(), &query("kept"), found(&[2]))
            .unwrap();

        store
            .keep_only_filters(&cpython(), vec![query("kept")])
            .unwrap();
        let read = reader.work_facts(&cpython()).unwrap().filters;
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read, BTreeMap::from([(query("kept"), found(&[2]))]));
    }

    #[test]
    fn a_stored_search_tsugiki_cant_understand_is_a_read_error() {
        let (store, open, reader, _path, folder) = opened("filter-bad-row");
        store
            .save_filter_results(&cpython(), &query("a"), found(&[1]))
            .unwrap();
        let _ = store
            .call(|connection| connection.execute("UPDATE filter_matches SET number = 'one'", []))
            .unwrap()
            .unwrap();

        let refused = reader.work_facts(&cpython()).unwrap_err();
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(refused.kind, ErrorKind::Internal);
    }
}
