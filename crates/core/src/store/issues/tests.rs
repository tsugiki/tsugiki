use super::super::testing::temp_database;
use super::super::*;
use crate::github::issues::IssueState;
use crate::github::labels::Label;
use crate::login::Login;
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
        labels: vec![],
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

fn labelled(number: u32, names: &[&str]) -> IssueFacts {
    IssueFacts {
        labels: names.iter().map(|name| Label::new(name)).collect(),
        ..issue(number)
    }
}

#[test]
fn an_issue_keeps_its_labels_and_a_save_replaces_them() {
    let read = saved(
        "issues-labels",
        &cpython(),
        vec![
            (
                cpython(),
                vec![
                    labelled(1, &["3.14", "type-bug"]),
                    labelled(2, &["awaiting", "Docs"]),
                ],
            ),
            (cpython(), vec![labelled(1, &["3.14", "type-feature"])]),
        ],
    );
    let names = |issue: &IssueFacts| -> Vec<String> {
        issue.labels.iter().map(|label| label.as_str().to_owned()).collect()
    };
    assert_eq!(
        read.iter().map(names).collect::<Vec<_>>(),
        [vec!["3.14", "type-feature"], vec!["awaiting", "Docs"]]
    );
}

#[test]
fn an_issue_stored_before_tsugiki_kept_labels_has_none_after_the_migration() {
    let (path, folder) = temp_database("issues-before-labels");
    let before = MIGRATIONS
        .iter()
        .position(|step| step.sql == super::LABELS)
        .unwrap();
    open_with(&path, &MIGRATIONS[..before]).unwrap().close();
    // A store of that age can't run today's save, which writes labels, so SQL stands in for it.
    let inserted = Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO issues (repository, number, title, author, updated_at, state)
             VALUES ('python/cpython', 1, 'Issue 1', 'some-author', 7, 'OPEN')",
            [],
        )
        .unwrap();
    assert_eq!(inserted, 1);

    let store = Store::open(&path).unwrap();
    let read = store.issues(&cpython()).unwrap();
    store.close();
    fs::remove_dir_all(folder).unwrap();
    assert_eq!(read, [issue(1)]);
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
