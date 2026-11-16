use std::collections::BTreeSet;

use time::{Duration, OffsetDateTime};

use super::super::testing::{migrated, temp_database};
use super::super::{Connection, MIGRATIONS, Store, issues, open_with, pull_requests};
use super::*;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{OffDefault, PrState};
use crate::links::{MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::seconds(seconds)
}

fn sha(text: &str) -> CommitSha {
    CommitSha::parse(text).unwrap()
}

fn pr(number: u32, title: &str) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: title.to_owned(),
        state: PrState::Open,
        is_draft: false,
        author: None,
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: at(i64::from(number)),
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: None,
        off_default: None,
    }
}

/// A PR on 3.14, titled `title`, whose description says `description` and names `picked` in its
/// cherry-pick lines.
fn on_branch(number: u32, title: &str, description: &str, picked: &[&str]) -> PullRequestFacts {
    PullRequestFacts {
        target_branch: "3.14".to_owned(),
        off_default: Some(OffDefault {
            cherry_picked_from: picked.iter().map(|text| sha(text)).collect(),
            ..OffDefault::described(description)
        }),
        ..pr(number, title)
    }
}

fn merged_as(number: u32, title: &str, commit: &str) -> PullRequestFacts {
    PullRequestFacts {
        state: PrState::Merged,
        merge_commit: Some(sha(commit)),
        ..pr(number, title)
    }
}

fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: None,
        assignees: vec![],
        labels: vec![],
        updated_at: at(i64::from(number)),
        state: Some(IssueState::Open),
    }
}

fn save(connection: &mut Connection, prs: &[PullRequestFacts]) {
    pull_requests::save(connection, &cpython(), prs, &[]).unwrap();
}

fn save_issues(connection: &mut Connection, numbers: &[u32]) {
    let held: Vec<IssueFacts> = numbers.iter().map(|number| issue(*number)).collect();
    issues::save(connection, &cpython(), &held, &[]).unwrap();
}

/// The links of PRs `numbers` under `conventions`, as each PR's number, fixes and Origin PR.
fn links(
    connection: &Connection,
    conventions: Conventions,
    numbers: &[u32],
) -> Vec<(u32, Vec<u32>, Option<u32>)> {
    let numbers = Numbers::of(numbers.iter().copied());
    let read = read(connection, &cpython(), conventions, &numbers).unwrap();
    let outline = read.into_iter().map(|links| {
        let fixes = links.fixes.into_iter().collect();
        (links.number, fixes, links.origin)
    });
    outline.collect()
}

#[test]
fn the_kept_links_are_those_the_link_rules_read_from_the_facts_under_each_conventions() {
    let mut connection = migrated();
    let prs = vec![
        merged_as(10, "gh-1: Fix it", "aaaa111"),
        on_branch(11, "[3.14] gh-1: Fix it (GH-10)", "", &[]),
        on_branch(
            12,
            "Fix it on 3.14",
            "Fixes #2\n\n(cherry picked from commit aaaa111)",
            &["aaaa111"],
        ),
        PullRequestFacts {
            closing_issues: vec![3, 99],
            ..pr(13, "gh-2: Fix it too")
        },
        on_branch(14, "[3.13] Fix it (GH-99)", "", &["bbbb222"]),
    ];
    save(&mut connection, &prs);
    save_issues(&mut connection, &[1, 2, 3]);
    let known: BTreeSet<u32> = [1, 2, 3].into();
    let merges = MergeCommits::of(&prs);

    for conventions in Conventions::ALL {
        let settings = RepositorySettings {
            conventions,
            ..RepositorySettings::new(cpython())
        };
        let ruled: Vec<(u32, Vec<u32>, Option<u32>)> = (prs.iter())
            .map(|facts| {
                let fixed = fixed_numbers(facts, &settings);
                let origin = backport_of(facts, conventions, &merges).map(|link| link.origin);
                (
                    facts.number,
                    fixed.intersection(&known).copied().collect(),
                    origin,
                )
            })
            .collect();
        assert_eq!(
            links(&connection, conventions, &[10, 11, 12, 13, 14]),
            ruled
        );
    }
}

#[test]
fn a_backport_saved_before_its_origin_pr_finds_it_once_the_origin_pr_is_saved() {
    let mut connection = migrated();
    let picking = on_branch(11, "Fix it on 3.14", "", &["aaaa111"]);
    save(&mut connection, &[picking]);
    let before = links(&connection, Conventions::Cpython, &[11]);

    save(&mut connection, &[merged_as(10, "Fix it", "aaaa1112222")]);

    assert_eq!(before, [(11, vec![], None)]);
    assert_eq!(
        links(&connection, Conventions::Cpython, &[11]),
        [(11, vec![], Some(10))],
        "a short SHA names the whole one it starts"
    );
}

#[test]
fn a_cherry_pick_line_names_the_first_of_its_commits_that_a_pr_became() {
    let mut connection = migrated();
    save(
        &mut connection,
        &[
            merged_as(10, "Fix it", "cccc333"),
            merged_as(20, "Fix it", "dddd444"),
            // A commit between two merge commits that starts neither names no PR.
            on_branch(11, "Fix it on 3.14", "", &["dddd444", "cccc000"]),
            // Nor does one past every merge commit.
            on_branch(12, "Fix it on 3.13", "", &["ffff999"]),
        ],
    );

    assert_eq!(
        links(&connection, Conventions::Cpython, &[11, 12]),
        [(11, vec![], Some(20)), (12, vec![], None)]
    );
}

#[test]
fn an_origin_pr_saved_again_without_the_commit_lets_its_backport_go() {
    let mut connection = migrated();
    save(
        &mut connection,
        &[
            merged_as(10, "Fix it", "aaaa111"),
            on_branch(11, "Fix it on 3.14", "", &["aaaa111"]),
        ],
    );
    let before = links(&connection, Conventions::Cpython, &[11]);

    save(&mut connection, &[pr(10, "Fix it")]);

    assert_eq!(before, [(11, vec![], Some(10))]);
    assert_eq!(
        links(&connection, Conventions::Cpython, &[11]),
        [(11, vec![], None)]
    );
}

#[test]
fn a_number_links_once_the_sync_fetches_it_as_an_issue() {
    let mut connection = migrated();
    save(&mut connection, &[pr(10, "gh-5: Fix it")]);
    let before = links(&connection, Conventions::Cpython, &[10]);

    save_issues(&mut connection, &[5]);

    assert_eq!(before, [(10, vec![], None)]);
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10]),
        [(10, vec![5], None)]
    );
}

#[test]
fn a_pr_saved_again_replaces_its_links() {
    let mut connection = migrated();
    save_issues(&mut connection, &[1, 2]);
    save(&mut connection, &[pr(10, "gh-1: Fix it")]);

    save(&mut connection, &[pr(10, "gh-2: Fix it")]);

    assert_eq!(
        links(&connection, Conventions::Cpython, &[10]),
        [(10, vec![2], None)]
    );
    let fixing = |issue: u32| {
        let issues = Numbers::of([issue]);
        fixing(&connection, &cpython(), Conventions::Cpython, &issues).unwrap()
    };
    assert_eq!(
        (fixing(1), fixing(2)),
        (BTreeSet::new(), BTreeSet::from([10]))
    );
}

#[test]
fn the_prs_naming_an_origin_pr_are_found_by_it_under_conventions_with_backports() {
    let mut connection = migrated();
    save(
        &mut connection,
        &[
            merged_as(10, "Fix it", "aaaa111"),
            on_branch(11, "[3.14] Fix it (GH-10)", "", &[]),
            on_branch(12, "Fix it on 3.13", "", &["aaaa111"]),
            on_branch(13, "[3.12] Fix it (GH-99)", "", &[]),
        ],
    );
    let naming = |conventions: Conventions| {
        let origins = Numbers::of([10]);
        naming_origins(&connection, &cpython(), conventions, &origins).unwrap()
    };

    assert_eq!(naming(Conventions::Cpython), BTreeSet::from([11, 12]));
    assert_eq!(naming(Conventions::Github), BTreeSet::new());
}

#[test]
fn the_step_that_makes_the_tables_fills_them_from_the_prs_the_store_holds() {
    let (path, _folder) = temp_database("links-filled");
    let before_links = &MIGRATIONS[..MIGRATIONS.len() - 1];
    open_with(&path, before_links).unwrap().close();
    let old = Connection::open(&path).unwrap();
    old.execute_batch(
        "INSERT INTO pull_requests (repository, number, title, state, is_draft, target_branch, updated_at,
                                    merge_commit)
             VALUES ('python/cpython', 10, 'gh-5: Fix it', 'MERGED', 0, 'main', 0, 'aaaa1112222');
         INSERT INTO closing_references (repository, number, issue) VALUES ('python/cpython', 10, 7);
         INSERT INTO pull_requests (repository, number, title, state, is_draft, target_branch, updated_at,
                                    off_default_description)
             VALUES ('python/cpython', 11, 'Fix it on 3.13', 'OPEN', 0, '3.13', 0, 'Fixes #6');
         INSERT INTO cherry_picks (repository, number, sha) VALUES ('python/cpython', 11, 'aaaa111');
         INSERT INTO issues (repository, number, title, updated_at, state)
             VALUES ('python/cpython', 5, 'It broke', 0, 'OPEN'),
                    ('python/cpython', 6, 'It broke on 3.13', 0, 'OPEN'),
                    ('python/cpython', 7, 'It broke too', 0, 'OPEN');",
    )
    .unwrap();
    drop(old);

    Store::open(&path).unwrap().close();

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5, 7], None), (11, vec![6], Some(10))]
    );
}

/// Opens the store at `path` once its links say version `version` of the rules worked them out, with only a
/// number among them that the rules never gave.
fn open_with_old_links(path: &std::path::Path, version: u32) {
    let old = Connection::open(path).unwrap();
    old.execute_batch(&format!(
        "DELETE FROM pr_fixes;
         DELETE FROM pr_origins;
         INSERT INTO pr_fixes (repository, number, issue, under) VALUES ('python/cpython', 10, 6, 'every');
         UPDATE link_rules SET version = {version};"
    ))
    .unwrap();
    drop(old);
    Store::open(path).unwrap().close();
}

#[test]
fn a_store_works_its_links_out_again_as_it_opens_after_the_rules_change() {
    let (path, _folder) = temp_database("links-stale");
    let store = Store::open(&path).unwrap();
    let saved = vec![
        merged_as(10, "gh-5: Fix it", "aaaa1112222"),
        on_branch(11, "Fix it on 3.13", "", &["aaaa111"]),
    ];
    store.save_pull_requests(&cpython(), saved).unwrap();
    store
        .save_issues(&cpython(), vec![issue(5), issue(6)])
        .unwrap();
    store.close();

    open_with_old_links(&path, RULES_VERSION - 1);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5], None), (11, vec![], Some(10))]
    );
}

#[test]
fn a_store_whose_links_this_version_worked_out_keeps_them_as_it_opens() {
    let (path, _folder) = temp_database("links-current");
    let store = Store::open(&path).unwrap();
    store
        .save_pull_requests(
            &cpython(),
            vec![merged_as(10, "gh-5: Fix it", "aaaa1112222")],
        )
        .unwrap();
    store
        .save_issues(&cpython(), vec![issue(5), issue(6)])
        .unwrap();
    store.close();

    open_with_old_links(&path, RULES_VERSION);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10]),
        [(10, vec![6], None)]
    );
}

/// The text each of `all` leaves in a column, once it has read back as itself.
fn stored_texts<T: ToSql + rusqlite::types::FromSql + std::fmt::Debug + PartialEq>(
    all: &[T],
) -> Vec<String> {
    let connection = Connection::open_in_memory().unwrap();
    let through = |value: &T| {
        let (text, read): (String, T) = connection
            .query_row(
                "SELECT :value AS value",
                named_params! { ":value": value },
                |row| Ok((row.get("value")?, row.get("value")?)),
            )
            .unwrap();
        assert_eq!(&read, value);
        text
    };
    all.iter().map(through).collect()
}

#[test]
fn each_link_keeps_the_text_earlier_versions_stored() {
    assert_eq!(stored_texts(Under::ALL), ["every", "cpython", "github"]);
    assert_eq!(stored_texts(FoundIn::ALL), ["cpython", "cherry_pick"]);
}

#[test]
fn the_tables_refuse_a_spelling_no_link_has() {
    let mut connection = migrated();
    save(&mut connection, &[pr(10, "Fix it")]);
    let refused = |sql: &str| connection.execute(sql, []).is_err();
    assert!(refused(
        "INSERT INTO pr_fixes VALUES ('python/cpython', 10, 1, 'cherry_pick')"
    ));
    assert!(refused(
        "INSERT INTO pr_origins VALUES ('python/cpython', 10, 'github', 9)"
    ));
    assert!(refused(
        "INSERT INTO pr_origins VALUES ('python/cpython', 10, 'every', 9)"
    ));
}

#[test]
fn a_title_s_marker_has_a_place_under_each_conventions_with_backports() {
    let kept = Conventions::ALL.map(|conventions| FoundIn::marker_of(conventions).is_some());
    assert_eq!(kept, Conventions::ALL.map(Conventions::has_backports));
}

/// What in SQLite's plan of `sql` reads every row of a Repository's, or of a table, other than the one row
/// of `first_syncs` a Repository has: each such step's detail.
fn whole_reads(connection: &Connection, sql: &str) -> Vec<String> {
    let mut plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    let mut rows = plan.raw_query();
    let mut found = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        let detail: String = row.get("detail").unwrap();
        // A scan of a `json_each` list reads only the numbers asked for.
        let scan = detail.starts_with("SCAN ") && !detail.contains("VIRTUAL TABLE");
        let whole = detail.ends_with("(repository=?)") && !detail.contains("first_syncs");
        if scan || whole {
            found.push(detail);
        }
    }
    found
}

#[test]
fn each_read_and_write_of_the_links_finds_its_rows_by_a_key_or_an_index() {
    let mut connection = migrated();
    save_issues(&mut connection, &[1]);
    save(
        &mut connection,
        &[
            on_branch(11, "[3.14] gh-1: Fix it (GH-10)", "", &["aaaa111"]),
            on_branch(12, "Fix it on 3.13", "", &["aaaa111"]),
        ],
    );
    let (seen, statements) = std::sync::mpsc::channel();
    let trace = move |event: rusqlite::trace::TraceEvent<'_>| {
        if let rusqlite::trace::TraceEvent::Stmt(_, sql) = event {
            seen.send(sql.to_owned()).unwrap();
        }
    };
    let codes = rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT;
    connection.trace_v2(codes, Some(trace)).unwrap();

    save(
        &mut connection,
        &[merged_as(10, "gh-1: Fix it", "aaaa1112222")],
    );
    let (name, listed) = (cpython(), Numbers::of([10, 11]));
    let conventions = Conventions::Cpython;
    let linked = read(&connection, &name, conventions, &listed).unwrap();
    let fixing = fixing(&connection, &name, conventions, &Numbers::of([1])).unwrap();
    let naming = naming_origins(&connection, &name, conventions, &listed).unwrap();
    let whole = pull_requests::read(&connection, &name, super::super::Items::Numbered(&listed));
    let before = Updates::Before {
        before: at(100),
        at_least: 1,
    };
    let walked = updated(&connection, &name, before).unwrap();
    connection
        .trace_v2(codes, None::<fn(rusqlite::trace::TraceEvent<'_>)>)
        .unwrap();

    assert_eq!(
        (
            linked.len(),
            fixing.len(),
            naming.len(),
            whole.unwrap().len(),
            walked.len()
        ),
        (2, 2, 2, 2, 1)
    );
    for sql in statements.try_iter() {
        assert_eq!(whole_reads(&connection, &sql), [""; 0], "{sql}");
    }
}

#[test]
fn a_walk_reads_at_least_what_it_asks_for_and_the_rest_of_the_last_second() {
    let mut connection = migrated();
    let prs: Vec<PullRequestFacts> = [(1, 50), (2, 40), (3, 40), (4, 30), (5, 20)]
        .into_iter()
        .map(|(number, seconds)| PullRequestFacts {
            updated_at: at(seconds),
            ..pr(number, "Fix it")
        })
        .collect();
    save(&mut connection, &prs);
    let numbers = |updates: Updates| -> Vec<u32> {
        let read = updated(&connection, &cpython(), updates).unwrap();
        read.into_iter().map(|(number, _)| number).collect()
    };

    let before = |seconds: i64, at_least: u32| Updates::Before {
        before: at(seconds),
        at_least,
    };
    assert_eq!(
        numbers(before(50, 1)),
        [3, 2],
        "the second of the last one comes whole"
    );
    assert_eq!(
        numbers(before(40, 9)),
        [4, 5],
        "fewer than asked means none are left"
    );
    assert_eq!(numbers(Updates::Since(at(40))), [1, 3, 2]);
}
