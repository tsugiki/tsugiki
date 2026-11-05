//! The names of the checks that failed on the user's PRs, read apart from the PRs because each PR runs too
//! many checks to ask about them all with every fetch.

use std::collections::BTreeMap;

use rusqlite::{Connection, named_params};
use time::OffsetDateTime;

use crate::github::Login;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE failed_checks (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (repository, number, name)
    ) WITHOUT ROWID;
    ALTER TABLE pull_requests ADD COLUMN checks_read_at INTEGER;
    CREATE INDEX activity_by_person ON activity (repository, who);
";

const SELECT_UNREAD: &str = "SELECT number FROM pull_requests
     WHERE repository = :repository AND author = :viewer AND state = 'OPEN'
         AND ci IN ('FAILURE', 'ERROR')
         AND (checks_read_at IS NULL OR ci_finished_at >= checks_read_at)
     ORDER BY number";

const SELECT_ALL: &str =
    "SELECT number, name FROM failed_checks WHERE repository = :repository ORDER BY number, name";

/// The numbers of `viewer`'s open PRs whose CI failed after Tsugiki last asked which checks failed. A failure
/// in the second Tsugiki asked counts, since asking twice costs little and a stale name misleads. Saving a PR
/// that moved forgets the last asking too, because a push can fail again with no pending state in between.
pub(super) fn unread(
    connection: &Connection,
    repository: &RepositoryName,
    viewer: &Login,
) -> rusqlite::Result<Vec<u32>> {
    let mut statement = connection.prepare(SELECT_UNREAD)?;
    let asked = named_params! { ":repository": repository, ":viewer": viewer };
    let rows = statement.query_map(asked, |row| row.get("number"))?;
    rows.collect()
}

fn save_one(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    names: &[String],
) -> rusqlite::Result<()> {
    let pull_request = named_params! { ":repository": repository, ":number": number };
    connection
        .execute(
            "DELETE FROM failed_checks WHERE repository = :repository AND number = :number",
            pull_request,
        )
        .map(drop)?;
    for name in names {
        connection
            .execute(
                "INSERT INTO failed_checks (repository, number, name)
                 VALUES (:repository, :number, :name)",
                named_params! { ":repository": repository, ":number": number, ":name": name },
            )
            .map(drop)?;
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    connection
        .execute(
            "UPDATE pull_requests SET checks_read_at = :now
             WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number, ":now": now },
        )
        .map(drop)
}

/// Replaces what the store knows about each PR's failed checks.
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    read: &[(u32, Vec<String>)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for (number, names) in read {
        save_one(&transaction, repository, *number, names)?;
    }
    transaction.commit()
}

/// Each PR's failed checks, by name.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<String>>> {
    let mut statement = connection.prepare(SELECT_ALL)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut found: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let name = row.get("name")?;
        found.entry(row.get("number")?).or_default().push(name);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime};

    use crate::github::Login;
    use crate::github::pull_requests::{CiState, PrState, PullRequestFacts};
    use crate::repository::RepositoryName;
    use crate::store::testing::temp_database;
    use crate::store::{Store, reader};

    fn me() -> Login {
        Login::new("octocat")
    }

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn failing(number: u32) -> PullRequestFacts {
        PullRequestFacts {
            number,
            title: format!("PR {number}"),
            state: PrState::Open,
            is_draft: false,
            author: Some(me()),
            target_branch: "main".to_owned(),
            ci: Some(CiState::Failure),
            updated_at: OffsetDateTime::now_utc() - Duration::hours(1),
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
            closing_issues: vec![],
            merge_commit: None,
            off_default: None,
        }
    }

    fn opened(name: &str) -> (Store, std::path::PathBuf, std::path::PathBuf) {
        let (path, folder) = temp_database(&format!("failed-checks-{name}"));
        (Store::open(&path).unwrap(), path, folder)
    }

    fn unread(store: &Store) -> Vec<u32> {
        store
            .pull_requests_with_unread_checks(&cpython(), &me())
            .unwrap()
    }

    #[test]
    fn only_the_users_open_prs_whose_ci_failed_are_worth_asking_about() {
        let (store, _path, folder) = opened("which");
        store
            .save_pull_requests(
                &cpython(),
                vec![
                    failing(1),
                    PullRequestFacts {
                        author: Some(Login::new("some-author")),
                        ..failing(2)
                    },
                    PullRequestFacts {
                        state: PrState::Merged,
                        ..failing(3)
                    },
                    PullRequestFacts {
                        ci: Some(CiState::Success),
                        ..failing(4)
                    },
                    PullRequestFacts {
                        ci: Some(CiState::Error),
                        ..failing(5)
                    },
                ],
            )
            .unwrap();

        assert_eq!(unread(&store), [1, 5]);
        store.close();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn asking_once_is_enough_and_the_names_replace_the_last_ones() {
        let (store, path, folder) = opened("once");
        store
            .save_pull_requests(&cpython(), vec![failing(1)])
            .unwrap();
        let first = vec!["Docs".to_owned(), "Tests".to_owned()];
        store
            .save_failed_checks(&cpython(), vec![(1, first)])
            .unwrap();
        assert_eq!(unread(&store), [0_u32; 0]);

        let then = vec!["Tests".to_owned()];
        store
            .save_failed_checks(&cpython(), vec![(1, then.clone())])
            .unwrap();
        let (to_open, reader) = reader();
        let open = to_open.open(&path).unwrap();
        let facts = reader.work_facts(&cpython()).unwrap();

        assert_eq!(facts.activity.failed_checks.get(&1), Some(&then));
        open.close();
        store.close();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_pr_that_moved_while_failing_is_worth_asking_about_again() {
        let (store, _path, folder) = opened("moved");
        store
            .save_pull_requests(&cpython(), vec![failing(1)])
            .unwrap();
        store
            .save_failed_checks(&cpython(), vec![(1, vec!["Docs".to_owned()])])
            .unwrap();
        store
            .save_pull_requests(
                &cpython(),
                vec![PullRequestFacts {
                    updated_at: OffsetDateTime::now_utc(),
                    ..failing(1)
                }],
            )
            .unwrap();

        assert_eq!(unread(&store), [1]);
        store.close();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_later_failure_is_worth_asking_about_again() {
        let (store, _path, folder) = opened("again");
        store
            .save_pull_requests(&cpython(), vec![failing(1)])
            .unwrap();
        store
            .save_failed_checks(&cpython(), vec![(1, vec!["Docs".to_owned()])])
            .unwrap();
        store
            .save_pull_requests(
                &cpython(),
                vec![PullRequestFacts {
                    ci: Some(CiState::Pending),
                    ..failing(1)
                }],
            )
            .unwrap();
        store
            .save_pull_requests(&cpython(), vec![failing(1)])
            .unwrap();

        assert_eq!(unread(&store), [1]);
        store.close();
        std::fs::remove_dir_all(folder).unwrap();
    }
}
