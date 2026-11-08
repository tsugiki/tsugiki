//! The Issues that PRs name, as facts (ADR 0010). The link from a PR to an Issue isn't stored: the core works
//! it out from a PR's facts when a screen asks (ADR 0011).

use std::collections::BTreeMap;

use rusqlite::{Connection, named_params};

use super::columns::UnixTime;
use super::{activity, pull_requests};
use crate::github::Login;
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
    CREATE TABLE issue_assignees (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login),
        FOREIGN KEY (repository, number) REFERENCES issues (repository, number)
    ) WITHOUT ROWID;
";

/// Whether each Issue is open, and how it closed (#188).
pub(super) const STATES: &str = "ALTER TABLE issues ADD COLUMN state TEXT;";

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
        forget_assignees(&transaction, repository, issue.number)?;
        for login in &issue.assignees {
            save_assignee(&transaction, repository, issue.number, login)?;
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
            "INSERT INTO issues (repository, number, title, author, updated_at, state)
             VALUES (:repository, :number, :title, :author, :updated_at, :state)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 author = excluded.author,
                 updated_at = excluded.updated_at,
                 state = excluded.state",
            named_params! {
                ":repository": repository,
                ":number": issue.number,
                ":title": issue.title,
                ":author": issue.author,
                ":updated_at": issue.updated_at.unix_timestamp(),
                ":state": issue.state,
            },
        )
        .map(drop)
}

fn forget_assignees(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM issue_assignees WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_assignee(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    login: &Login,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO issue_assignees (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn assignees(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    let query = "SELECT number, login FROM issue_assignees WHERE repository = :repository";
    pull_requests::values_by_number(
        connection.prepare(query)?,
        named_params! { ":repository": repository },
        "login",
    )
}

/// The Issues of `repository` that Tsugiki keeps.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<IssueFacts>> {
    let query = "SELECT number, title, author, updated_at, state FROM issues
                 WHERE repository = :repository
                 ORDER BY number";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut assignees = assignees(connection, repository)?;
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
            state: row.get("state")?,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::super::testing::temp_database;
    use super::super::*;
    use crate::github::Login;
    use crate::github::issues::IssueState;
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
            state: Some(IssueState::Open),
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
    fn each_state_reads_back_the_same() {
        let states = [
            IssueState::Open,
            IssueState::Closed,
            IssueState::NotPlanned,
            IssueState::Duplicate,
        ];
        let issues: Vec<IssueFacts> = states
            .into_iter()
            .zip(1..)
            .map(|(state, number)| IssueFacts {
                state: Some(state),
                ..issue(number)
            })
            .collect();
        let read = saved(
            "issues-states",
            &cpython(),
            vec![(cpython(), issues.clone())],
        );
        assert_eq!(read, issues);
    }

    #[test]
    fn an_issue_saved_before_tsugiki_kept_states_has_none() {
        let (path, folder) = temp_database("issues-no-state");
        let store = Store::open(&path).unwrap();
        let closed = IssueFacts {
            state: Some(IssueState::Closed),
            ..issue(1)
        };
        store.save_issues(&cpython(), vec![closed]).unwrap();
        store.close();
        // Only SQL can leave the column empty, as the migration leaves the rows saved before it.
        let emptied = Connection::open(&path)
            .unwrap()
            .execute("UPDATE issues SET state = NULL", [])
            .unwrap();
        assert_eq!(emptied, 1);

        let store = Store::open(&path).unwrap();
        let read = store.issues(&cpython()).unwrap();
        store.close();
        fs::remove_dir_all(folder).unwrap();
        let unknown = IssueFacts {
            state: None,
            ..issue(1)
        };
        assert_eq!(read, [unknown]);
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
