//! The Issues that PRs name, as facts (ADR 0010). The link from a PR to an Issue isn't stored: the core works
//! it out from a PR's facts when a screen asks (ADR 0011).

use rusqlite::{Connection, named_params};

use super::columns::UnixTime;
use super::{activity, pull_requests};
use crate::github::activity::ItemActivity;
use crate::github::issues::IssueFacts;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE issues (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        title TEXT NOT NULL,
        author TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
";

/// Stores `issues` in one transaction, each replacing what Tsugiki knew about it (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    issues: &[IssueFacts],
    events: &[ItemActivity],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for issue in issues {
        save_issue(&transaction, repository, issue)?;
        pull_requests::forget_assignees(&transaction, repository, issue.number)?;
        for login in &issue.assignees {
            pull_requests::save_assignee(&transaction, repository, issue.number, login)?;
        }
    }
    activity::save(&transaction, repository, events)?;
    transaction.commit()
}

fn save_issue(
    connection: &Connection,
    repository: &RepositoryName,
    issue: &IssueFacts,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issues (repository, number, title, author, updated_at)
             VALUES (:repository, :number, :title, :author, :updated_at)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 author = excluded.author,
                 updated_at = excluded.updated_at",
            named_params! {
                ":repository": repository,
                ":number": issue.number,
                ":title": issue.title,
                ":author": issue.author,
                ":updated_at": issue.updated_at.unix_timestamp(),
            },
        )
        .map(drop)
}

/// The Issues of `repository` that Tsugiki keeps.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<IssueFacts>> {
    let query = "SELECT number, title, author, updated_at FROM issues
                 WHERE repository = :repository
                 ORDER BY number";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut assignees = pull_requests::assignees(connection, repository)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let UnixTime(updated_at) = row.get("updated_at")?;
        let number = row.get("number")?;
        found.push(IssueFacts {
            number,
            title: row.get("title")?,
            author: row.get("author")?,
            assignees: assignees.remove(&number).unwrap_or_default(),
            updated_at,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::super::testing::temp_database;
    use super::super::*;
    use crate::github::Login;
    use time::OffsetDateTime;

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn issue(number: u32) -> IssueFacts {
        IssueFacts {
            number,
            title: format!("Issue {number}"),
            author: Some(Login::new("some-author")),
            assignees: vec![],
            updated_at: OffsetDateTime::from_unix_timestamp(7).unwrap(),
        }
    }

    /// The Issues `saves` leave in the store, as the reader sees them for `repository`.
    fn saved(
        name: &str,
        repository: &RepositoryName,
        saves: Vec<(RepositoryName, Vec<IssueFacts>)>,
    ) -> Vec<IssueFacts> {
        let (path, folder) = temp_database(name);
        let store = Store::open(&path).unwrap();
        let (to_open, reader) = reader();
        let open = to_open.open(&path).unwrap();
        for (repository, issues) in saves {
            store.save_issues(&repository, issues).unwrap();
        }
        let read = reader.work_facts(repository).unwrap();
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();
        read.issues
    }

    #[test]
    fn a_database_a_failed_run_left_behind_is_gone_for_the_next_test() {
        // A test that panics leaves its folder behind, and a later run can get the same process ID.
        let (path, _) = temp_database("issues-leftover");
        let store = Store::open(&path).unwrap();
        store.save_issues(&cpython(), vec![issue(1)]).unwrap();
        store.close();

        let (path, folder) = temp_database("issues-leftover");
        let store = Store::open(&path).unwrap();
        let read = store.issues(&cpython()).unwrap();
        store.close();
        fs::remove_dir_all(folder).unwrap();
        assert_eq!(read, []);
    }

    #[test]
    fn what_the_writer_saves_the_reader_reads_in_order() {
        let read = saved(
            "issues-roundtrip",
            &cpython(),
            vec![(cpython(), vec![issue(2), issue(1)])],
        );
        assert_eq!(read, [issue(1), issue(2)]);
    }

    #[test]
    fn an_issue_with_no_author_reads_back_the_same() {
        let sparse = IssueFacts {
            author: None,
            ..issue(1)
        };
        let read = saved(
            "issues-sparse",
            &cpython(),
            vec![(cpython(), vec![sparse.clone()])],
        );
        assert_eq!(read, [sparse]);
    }

    #[test]
    fn saving_an_issue_again_replaces_what_was_known() {
        let changed = IssueFacts {
            title: "A better title".to_owned(),
            author: None,
            assignees: vec![],
            updated_at: OffsetDateTime::from_unix_timestamp(8).unwrap(),
            ..issue(1)
        };
        let read = saved(
            "issues-replace",
            &cpython(),
            vec![
                (cpython(), vec![issue(1)]),
                (cpython(), vec![changed.clone()]),
            ],
        );
        assert_eq!(read, [changed]);
    }

    #[test]
    fn repositories_keep_apart_and_their_names_ignore_case() {
        let peps = RepositoryName::parse("python/peps").unwrap();
        let shouting = RepositoryName::parse("Python/CPython").unwrap();
        let read = saved(
            "issues-repositories",
            &shouting,
            vec![(cpython(), vec![issue(1)]), (peps, vec![issue(2)])],
        );
        assert_eq!(read, [issue(1)]);
    }

    #[test]
    fn a_save_that_fails_says_so() {
        let refused = Store::stopped()
            .save_issues(&cpython(), vec![issue(1)])
            .unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Internal);
    }

    #[test]
    fn a_time_tsugiki_cant_understand_is_a_read_error() {
        let (path, folder) = temp_database("issues-bad-time");
        let store = Store::open(&path).unwrap();
        let (to_open, reader) = reader();
        let open = to_open.open(&path).unwrap();
        store.save_issues(&cpython(), vec![issue(1)]).unwrap();
        let _ = store
            .call(|connection| {
                connection.execute("UPDATE issues SET updated_at = 9223372036854775807", [])
            })
            .unwrap()
            .unwrap();

        let refused = reader.work_facts(&cpython()).unwrap_err();
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(refused, Error::new(ErrorKind::Internal, CANT_READ));
    }
}
