use super::file::{Contents, contents};
use super::testing::{V1, V2, backups, has_table, temp_database, user_version};
use super::*;
use crate::github::CommitSha;
use crate::github::issue_content::IssueComment;
use crate::github::issues::IssueState;
use crate::github::pull_request_content::{
    Anchor, DiffHunk, Opening, Resolution, Review, Side, Subject, Submission, ThreadComment,
    ThreadId, ThreadLine,
};
use crate::github::pull_requests::{CiState, LatestReview, OffDefault, PrState, ReviewState};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use time::OffsetDateTime;

#[test]
fn a_new_database_gets_the_whole_schema_and_no_backup() {
    let (path, folder) = temp_database("new");
    Store::open(&path).unwrap().close();

    let latest = MIGRATIONS.last().unwrap().version;
    assert_eq!(user_version(&path), latest);
    assert!(has_table(&path, "viewer"));
    assert_eq!(backups(&folder), [""; 0]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_store_waits_a_moment_for_a_lock_that_is_letting_go() {
    let (path, folder) = temp_database("letting-go");
    let first = open_with(&path, &[V1]).unwrap();
    let closing = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        first.close();
    });
    let started = Instant::now();

    let second = open_with(&path, &[V1]);

    let waited = started.elapsed();
    closing.join().unwrap();
    second.unwrap().close();
    fs::remove_dir_all(folder).unwrap();
    assert!(waited >= Duration::from_millis(100), "{waited:?}");
}

#[test]
fn a_second_store_on_the_same_file_is_refused_and_leaves_it_alone() {
    let (path, folder) = temp_database("held");
    let first = open_with(&path, &[V1]).unwrap();

    let problem = open_with(&path, &[V1, V2]).err().unwrap();

    first.close();
    assert_eq!(problem.message, IN_USE);
    assert_eq!(user_version(&path), 1);
    assert_eq!(backups(&folder), [""; 0]);
    open_with(&path, &[V1, V2]).unwrap().close();
    assert_eq!(
        user_version(&path),
        2,
        "closing the first store lets go of the file"
    );
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_folder_is_created_when_it_is_missing() {
    let (_, folder) = temp_database("missing-folder");
    let path = folder.join("below").join("tsugiki.db");
    Store::open(&path).unwrap().close();
    assert!(path.exists());
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_database_runs_in_wal_mode() {
    let (path, folder) = temp_database("wal");
    Store::open(&path).unwrap().close();

    let mode: String = Connection::open(&path)
        .unwrap()
        .pragma_query_value(None, "journal_mode", |row| row.get("journal_mode"))
        .unwrap();
    fs::remove_dir_all(folder).unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn nobody_is_remembered_at_first() {
    let (path, folder) = temp_database("first");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.last_login().unwrap(), None);
    store.close();
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_login_survives_a_restart_and_the_newest_one_wins() {
    let (path, folder) = temp_database("restart");
    let store = Store::open(&path).unwrap();
    store.remember_login(&Login::new("octocat")).unwrap();
    store.remember_login(&Login::new("hubot")).unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    assert_eq!(store.last_login().unwrap(), Some(Login::new("hubot")));
    store.close();
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_migration_reaches_the_file_while_the_store_is_still_open() {
    let (path, folder) = temp_database("checkpoint");
    open_with(&path, &[V1]).unwrap().close();

    // A crash or a force-kill skips closing the store, so what the header says can't wait for `close`.
    let store = open_with(&path, &[V1, V2]).unwrap();
    assert_eq!(contents(&path), Contents::Database { version: 2 });

    store.close();
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_new_database_reaches_the_file_while_the_store_is_still_open() {
    let (path, folder) = temp_database("checkpoint-new");
    let store = open_with(&path, &[V1]).unwrap();
    assert_eq!(contents(&path), Contents::Database { version: 1 });
    store.close();
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_folder_that_cant_be_created_is_an_error() {
    let (_, folder) = temp_database("blocked-folder");
    fs::write(folder.join("a-file"), "").unwrap();
    let path = folder.join("a-file").join("tsugiki.db");

    let problem = Store::open(&path).err().unwrap();

    assert_eq!(problem.kind, ErrorKind::Internal);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_panic_on_the_database_thread_comes_back_as_an_error() {
    let store = Store::open(Path::new(":memory:")).unwrap();

    let panicked = store.call(|_| panic!("a panic on the database thread"));
    let after = store.last_login();
    store.close();

    let stopped = Error::new(ErrorKind::Internal, STOPPED);
    assert_eq!(panicked.unwrap_err(), stopped);
    assert_eq!(after.unwrap_err(), stopped);
}

/// Tables that stay a few rows long, so reading all of them is fine. Any other table counts as large,
/// so a new one is checked until someone says otherwise.
const SMALL_TABLES: [&str; 1] = ["viewer"];

/// The tables that `sql` reads end to end, other than `SMALL_TABLES`.
fn full_scans(connection: &Connection, sql: &str) -> Vec<String> {
    let mut plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    let mut rows = plan.raw_query();
    let mut scans = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        let detail: String = row.get("detail").unwrap();
        let table = detail
            .strip_prefix("SCAN ")
            .and_then(|rest| rest.split(' ').next());
        if let Some(table) = table.filter(|table| !SMALL_TABLES.contains(table)) {
            scans.push(table.to_owned());
        }
    }
    scans
}

/// Each statement the reads a screen waits on run, as SQLite traces them. One row in each table makes a
/// query inside a loop over rows run too. A query behind a condition these rows don't meet stays unseen.
fn view_statements(connection: &mut Connection) -> Vec<String> {
    let cpython = RepositoryName::parse("python/cpython").unwrap();
    let viewer = Login::new("octocat");
    let saved_at = OffsetDateTime::UNIX_EPOCH;
    let pull_request = PullRequestFacts {
        number: 1,
        title: "gh-1: Fix it".to_owned(),
        state: PrState::Open,
        is_draft: false,
        author: Some(Login::new("some-author")),
        target_branch: "main".to_owned(),
        ci: Some(CiState::Success),
        updated_at: saved_at,
        review_requests: vec![viewer.clone()],
        latest_reviews: vec![LatestReview {
            author: Login::new("some-reviewer"),
            state: ReviewState::Approved,
        }],
        assignees: vec![Login::new("some-assignee")],
        labels: vec![],
        closing_issues: vec![7],
        merge_commit: CommitSha::parse("0123abc"),
        base: None,
        head: None,
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: CommitSha::parse("0123abc").into_iter().collect(),
            committers: vec![Some(viewer.clone())],
        }),
    };
    let issue = IssueFacts {
        number: 1,
        title: "It broke".to_owned(),
        author: Some(Login::new("some-author")),
        assignees: vec![],
        labels: vec![],
        updated_at: saved_at,
        state: Some(IssueState::Open),
    };
    write_last_login(connection, &viewer).unwrap();
    pull_requests::save(connection, &cpython, &[pull_request], &[]).unwrap();
    issues::save(connection, &cpython, &[issue], &[]).unwrap();
    let query: FilterQuery = "is:open".to_owned().try_into().unwrap();
    let results = FilterResults {
        numbers: [1].into(),
        problem: Some(FilterProblem::Refused("It was cut off".to_owned())),
    };
    filters::save(connection, &cpython, &query, &results).unwrap();
    // With nothing kept, the read would stop before it asks for the comments.
    let content = IssueContent {
        body_html: "<p>It crashes</p>".to_owned(),
        comments: Vec::new(),
        earlier_comments: 0,
        opened_at: Some(saved_at),
    };
    let review = Review {
        author: Some(Login::new("some-reviewer")),
        state: ReviewState::Approved,
        commit: CommitSha::parse("0123abc"),
        submitted_at: saved_at,
        body_html: String::new(),
        hidden: None,
        threads: vec![ThreadLine {
            id: ThreadId::new("T1"),
            path: "Lib/ast.py".to_owned(),
            opening: Opening::Words {
                words: "Why?".to_owned(),
            },
            replies: 0,
            resolution: Resolution::Open,
            anchor: Anchor::Current,
            subject: Subject::File,
            comments: vec![ThreadComment {
                comment: IssueComment {
                    author: None,
                    created_at: saved_at,
                    body_html: "<p>Why?</p>".to_owned(),
                    hidden: None,
                },
                submission: Submission::Submitted,
            }],
            hunk: None,
        }],
    };
    // The user's draft, on a line, with the code it was written against.
    let drafted = ThreadLine {
        id: ThreadId::new("T2"),
        path: "Lib/ast.py".to_owned(),
        opening: Opening::Words {
            words: "A draft".to_owned(),
        },
        replies: 0,
        resolution: Resolution::Open,
        anchor: Anchor::Current,
        subject: Subject::Line {
            side: Side::Right,
            line: Some(2),
            original: Some(2),
        },
        comments: vec![ThreadComment {
            comment: IssueComment {
                author: Some(Login::new("the-user")),
                created_at: saved_at,
                body_html: "<p>A draft</p>".to_owned(),
                hidden: None,
            },
            submission: Submission::Pending,
        }],
        hunk: DiffHunk::parse("@@ -1,2 +1,2 @@\n x\n+y"),
    };
    let reviewed = PullRequestContent {
        conversation: content.clone(),
        reviews: vec![review],
        earlier_reviews: 0,
        head: CommitSha::parse("0123abc"),
        pending: vec![drafted],
    };
    pull_request_content::save(connection, &cpython, 1, &reviewed).unwrap();
    let (seen, statements) = mpsc::channel();
    let trace = move |event: TraceEvent<'_>| {
        if let TraceEvent::Stmt(_, sql) = event {
            seen.send(sql.to_owned()).unwrap();
        }
    };
    let codes = TraceEventCodes::SQLITE_TRACE_STMT;
    connection.trace_v2(codes, Some(trace)).unwrap();
    let (read, kept) = read_issue_facts(connection, &cpython, 1).unwrap();
    assert_eq!(read.pull_requests.len(), 1);
    assert_eq!(kept, Some(content));
    let (_, kept) = read_pull_request_facts(connection, &cpython, 1).unwrap();
    assert_eq!(kept, Some(reviewed));
    let no_trace = None::<fn(TraceEvent<'_>)>;
    connection.trace_v2(codes, no_trace).unwrap();
    statements.try_iter().collect()
}

fn migrated() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    for step in MIGRATIONS {
        connection.execute_batch(step.sql).unwrap();
    }
    connection
}

#[test]
fn no_view_query_scans_a_large_table() {
    let mut connection = migrated();
    for sql in view_statements(&mut connection) {
        assert_eq!(full_scans(&connection, &sql), [""; 0], "{sql}");
    }
}

#[test]
fn the_plan_check_sees_each_table_a_view_reads() {
    let statements = view_statements(&mut migrated());
    let reads = |table: &str| {
        let from = format!("FROM {table}");
        statements.iter().any(|sql| sql.contains(&from))
    };
    let tables = [
        "viewer",
        "pull_requests",
        "review_requests",
        "latest_reviews",
        "assignees",
        "pull_request_labels",
        "closing_references",
        "cherry_picks",
        "committers",
        "issues",
        "issue_assignees",
        "issue_labels",
        "filter_matches",
        "filter_problems",
        "pr_reviews",
        "review_threads",
        "review_thread_comments",
        "marked_unseen",
    ];
    assert_eq!(tables.map(reads), [true; 18]);
}

#[test]
fn a_row_for_an_item_the_store_doesnt_hold_is_refused() {
    // Tsugiki's own saves write an item before its rows, so only SQL can write one without the other.
    let connection = migrated();
    let orphans = [
            "INSERT INTO review_requests VALUES ('python/cpython', 1, 'someone')",
            "INSERT INTO latest_reviews VALUES ('python/cpython', 1, 'someone', 'APPROVED')",
            "INSERT INTO assignees VALUES ('python/cpython', 1, 'someone')",
            "INSERT INTO closing_references VALUES ('python/cpython', 1, 7)",
            "INSERT INTO cherry_picks VALUES ('python/cpython', 1, '0123abc')",
            "INSERT INTO committers VALUES ('python/cpython', 1, 0, 'someone')",
            "INSERT INTO issue_assignees VALUES ('python/cpython', 7, 'someone')",
            "INSERT INTO issue_labels VALUES ('python/cpython', 7, 'type-bug')",
            "INSERT INTO pull_request_labels VALUES ('python/cpython', 1, 'skip news')",
            "INSERT INTO issue_comments VALUES ('python/cpython', 7, 0, 'someone', 0, '<p>Hi</p>', NULL)",
            "INSERT INTO pr_reviews VALUES ('python/cpython', 1, 0, 'someone', 'APPROVED', NULL, 0, '', NULL)",
            "INSERT INTO review_threads VALUES ('python/cpython', 1, 0, 0, 'T1', 'a.py', 1, 'RIGHT', 1, 'Hm', NULL, 0,
                                                'open', 'current', NULL)",
            "INSERT INTO review_thread_comments VALUES ('python/cpython', 1, 0, 0, 0, 'someone', 0, '<p>Hm</p>', NULL,
                                                        'submitted')",
        ];
    let refused = orphans.map(|sql| {
        let refusal = connection.execute(sql, []).err();
        refusal.and_then(|error| error.sqlite_error().map(|failure| failure.extended_code))
    });
    assert_eq!(
        refused,
        [Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY); 13]
    );
}

#[test]
fn a_store_with_nothing_to_migrate_enforces_foreign_keys() {
    let (path, folder) = temp_database("keys-enforced");
    open_with(&path, &[V1]).unwrap().close();

    let store = open_with(&path, &[V1]).unwrap();
    let enforced: bool = store
        .call(|connection| {
            connection.pragma_query_value(None, "foreign_keys", |row| row.get("foreign_keys"))
        })
        .unwrap()
        .unwrap();
    store.close();
    fs::remove_dir_all(folder).unwrap();
    assert!(enforced);
}

#[test]
fn the_plan_check_finds_a_full_scan_of_a_large_table() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE issues (id INTEGER PRIMARY KEY, title TEXT, author TEXT);
                 CREATE INDEX issues_by_author ON issues (author);
                 CREATE TABLE viewer (id INTEGER PRIMARY KEY, login TEXT);",
        )
        .unwrap();

    let no_index = "SELECT title FROM issues WHERE title = :title";
    let whole_index = "SELECT author FROM issues";
    assert_eq!(full_scans(&connection, no_index), ["issues"]);
    assert_eq!(full_scans(&connection, whole_index), ["issues"]);

    let by_key = "SELECT title FROM issues WHERE id = :id";
    let by_index = "SELECT title FROM issues WHERE author = :author";
    let small = "SELECT login FROM viewer";
    assert_eq!(full_scans(&connection, by_key), [""; 0]);
    assert_eq!(full_scans(&connection, by_index), [""; 0]);
    assert_eq!(full_scans(&connection, small), [""; 0]);
}
