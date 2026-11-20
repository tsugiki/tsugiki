use std::collections::BTreeSet;

use super::super::testing::{
    at_second, keep_old_links, kept_fixes, migrated, refuse_new_links, store_with_old_links,
    temp_database,
};
use super::super::{Connection, MIGRATIONS, Store, issues, open_with, pull_requests};
use super::*;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::{OffDefault, PrState};
use crate::links::{MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;
use crate::testing::{TempPath, cpython, gcmon, issue_facts, pr_facts, sha};

fn pr(number: u32, title: &str) -> PullRequestFacts {
    PullRequestFacts {
        title: title.to_owned(),
        updated_at: at_second(i64::from(number)),
        ..pr_facts(number)
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
        updated_at: at_second(i64::from(number)),
        ..issue_facts(number)
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
fn a_store_whose_links_no_rules_worked_out_leaves_them_to_the_refill_of_its_repositories() {
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

    let store = Store::open(&path).unwrap();
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
    refill_whole(&store, &cpython());
    store.keep_link_rules().unwrap();

    assert_eq!(store.stale_links().unwrap(), None);
    store.close();
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5, 7], None), (11, vec![6], Some(10))]
    );
}

/// The store at `path`, opened again once its links say version `version` of the rules worked them out,
/// with only a number among them that the rules never gave: PR 10 fixes Issue 6.
fn open_with_old_links(path: &std::path::Path, version: u32) -> Store {
    let store = Store::open(path).unwrap();
    keep_old_links(&store, version, &[(cpython(), 10, 6)]);
    store.close();
    Store::open(path).unwrap()
}

/// A store at a new path that holds PR 10, which fixes Issue 5, and Backport 11, which comes from it, with
/// Issues 5 and 6.
fn store_with_a_backport(name: &str) -> (std::path::PathBuf, TempPath) {
    let (path, folder) = temp_database(name);
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
    (path, folder)
}

/// Works out the links of every PR of `repository` that `store` holds again, in one batch.
fn refill_whole(store: &Store, repository: &RepositoryName) {
    assert_eq!(
        store.refill_links(repository, 0, NonZeroU32::MAX).unwrap(),
        None
    );
}

#[test]
fn a_refill_takes_a_repositorys_prs_a_batch_at_a_time_in_number_order() {
    let (path, _folder) = store_with_a_backport("links-batches");
    let store = open_with_old_links(&path, RULES_VERSION - 1);
    let connection = Connection::open(&path).unwrap();
    let refill = |from| {
        store
            .refill_links(&cpython(), from, NonZeroU32::MIN)
            .unwrap()
    };

    assert_eq!(refill(0), Some(11));
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5], None), (11, vec![], None)]
    );
    assert_eq!(refill(11), Some(12));
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5], None), (11, vec![], Some(10))]
    );
    assert_eq!(refill(12), None);
    store.close();
}

#[test]
fn a_store_opened_after_the_rules_change_answers_from_its_kept_links_until_they_refill() {
    let (path, _folder) = store_with_a_backport("links-stale");

    let store = open_with_old_links(&path, RULES_VERSION - 1);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![6], None), (11, vec![], None)]
    );
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
    refill_whole(&store, &cpython());
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10, 11]),
        [(10, vec![5], None), (11, vec![], Some(10))]
    );
    // The refill of each Repository comes first, so a crash before this refills them all again.
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
    store.keep_link_rules().unwrap();
    assert_eq!(store.stale_links().unwrap(), None);
    store.close();
}

#[test]
fn a_store_whose_links_this_version_worked_out_keeps_them_as_it_opens() {
    let (path, _folder) = store_with_a_backport("links-current");

    let store = open_with_old_links(&path, RULES_VERSION);

    assert_eq!(store.stale_links().unwrap(), None);
    store.close();
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        links(&connection, Conventions::Cpython, &[10]),
        [(10, vec![6], None)]
    );
}

#[test]
fn a_store_that_holds_no_pr_keeps_this_versions_rules_as_it_opens() {
    let (path, _folder) = temp_database("links-empty");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.stale_links().unwrap(), None);
    keep_old_links(&store, RULES_VERSION - 1, &[]);
    store.close();

    let store = Store::open(&path).unwrap();

    assert_eq!(store.stale_links().unwrap(), None);
    store.close();
}

#[test]
fn a_refill_works_out_only_its_own_repositorys_links() {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    let mut stale = store.stale_links().unwrap().unwrap();
    stale.sort_by_key(ToString::to_string);
    assert_eq!(stale, [cpython(), gcmon()]);

    refill_whole(&store, &gcmon());

    let (python, gcmon) = (cpython().to_string(), gcmon().to_string());
    assert_eq!(kept_fixes(&store), [(python, 10, 6), (gcmon, 10, 5)]);
}

#[test]
fn the_end_of_a_refill_forgets_the_links_of_names_that_spell_no_repository() {
    let (path, _folder) = store_with_a_backport("links-unparsed");
    let store = open_with_old_links(&path, RULES_VERSION - 1);
    store.close();
    let old = Connection::open(&path).unwrap();
    // One name that doesn't parse, and one that parses to python/cpython but isn't spelled so.
    for name in ["no-slash", "python/cpython.git"] {
        old.execute_batch(&format!(
            "INSERT INTO pull_requests (repository, number, title, state, is_draft, target_branch,
                                        updated_at)
                 VALUES ('{name}', 10, 'gh-5: Fix it', 'OPEN', 0, 'main', 0);
             INSERT INTO pr_fixes (repository, number, issue, under) VALUES ('{name}', 10, 6, 'every');"
        ))
        .unwrap();
    }
    drop(old);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));

    refill_whole(&store, &cpython());
    store.keep_link_rules().unwrap();

    assert_eq!(kept_fixes(&store), [(cpython().to_string(), 10, 5)]);
    store.close();
}

#[test]
fn a_refill_that_fails_keeps_the_links_it_found_and_their_version() {
    let (path, _folder) = store_with_a_backport("links-refused");
    let store = open_with_old_links(&path, RULES_VERSION - 1);
    refuse_new_links(&store);

    assert!(store.refill_links(&cpython(), 0, NonZeroU32::MAX).is_err());

    assert_eq!(kept_fixes(&store), [(cpython().to_string(), 10, 6)]);
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
    store.close();
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

/// The PRs that try `rule`, in the batches the store saves them in, and the rows `rule` alone keeps for
/// them, one line for each. A released rule keeps these rows: a change to them is a new rule (`LinkRule`).
fn released(rule: LinkRule) -> (Vec<Vec<PullRequestFacts>>, Vec<&'static str>) {
    match rule {
        LinkRule::ClosingReferences => (
            vec![vec![
                PullRequestFacts {
                    closing_issues: vec![2],
                    ..pr(1, "gh-1: Fix it")
                },
                PullRequestFacts {
                    closing_issues: vec![3, 4],
                    ..on_branch(2, "[3.14] gh-5: Fix it (GH-6)", "Fixes #7", &[])
                },
            ]],
            vec![
                "PR 1 fixes 2 under every",
                "PR 2 fixes 3 under every",
                "PR 2 fixes 4 under every",
            ],
        ),
        LinkRule::DescriptionKeywords => {
            let description = "Fixes #4\n`Closes #5` <!-- Fixes #6 -->\n```\nFixes #7\n```\n\
                               Resolves https://github.com/python/cpython/issues/8\n\
                               Fixes other/repo#9, see gh-10, closes GH-11";
            (
                vec![vec![PullRequestFacts {
                    closing_issues: vec![14],
                    ..on_branch(10, "[3.14] gh-12: Fix it (GH-13)", description, &[])
                }]],
                vec![
                    "PR 10 fixes 4 under every",
                    "PR 10 fixes 8 under every",
                    "PR 10 fixes 11 under every",
                ],
            )
        }
        LinkRule::CpythonTitleIssues => (
            vec![vec![
                pr(20, "gh-1: Fix it"),
                pr(21, "GH-3: Fix it"),
                pr(22, "[3.14] gh-12: Fix it (GH-13)"),
                on_branch(23, "[3.14] gh-12, gh-14: Fix it (GH-13)", "Fixes #15", &[]),
            ]],
            vec![
                "PR 20 fixes 1 under cpython",
                "PR 21 fixes 3 under cpython",
                "PR 22 fixes 12 under cpython",
                "PR 23 fixes 12 under cpython",
                "PR 23 fixes 14 under cpython",
            ],
        ),
        LinkRule::CpythonTitleOrigin => (
            vec![vec![
                on_branch(30, "[3.14] gh-12: Fix it (GH-13)", "", &[]),
                // On the default branch.
                pr(31, "[3.14] gh-12: Fix it (GH-13)"),
                on_branch(32, "[3.13] Fix it (#15)", "", &[]),
            ]],
            vec!["PR 30 comes from 13 by cpython"],
        ),
        LinkRule::CherryPickLine => (
            vec![
                vec![
                    // Saved before the PR its line names.
                    on_branch(40, "Fix it on 3.13", "", &["bbbb222"]),
                    // The first of its commits, in the order of their SHAs, that a PR became.
                    on_branch(41, "Fix it on 3.12", "", &["aaaa111", "cccc333"]),
                    // A merge commit that sorts after the line's commit without starting with it.
                    on_branch(42, "Fix it on 3.11", "", &["dddd444"]),
                    on_branch(43, "[3.14] Fix it (GH-30)", "", &["bbbb222"]),
                ],
                vec![
                    merged_as(50, "Fix it", "bbbb2223333"),
                    merged_as(51, "Fix it", "cccc3334444"),
                    merged_as(52, "Fix it", "dddd5555"),
                ],
            ],
            vec![
                "PR 40 comes from 50 by cherry_pick",
                "PR 41 comes from 51 by cherry_pick",
                "PR 43 comes from 50 by cherry_pick",
            ],
        ),
    }
}

/// A store that saved `batches` in turn, under `LinkRule::SET`.
fn saved(batches: &[Vec<PullRequestFacts>]) -> Connection {
    let mut connection = migrated();
    for batch in batches {
        save(&mut connection, batch);
    }
    connection
}

/// The rows `rule` alone keeps for the PRs of `batches`, once the store holds them all.
fn kept_by(rule: LinkRule, batches: &[Vec<PullRequestFacts>]) -> Vec<String> {
    let connection = saved(batches);
    (connection.execute_batch("DELETE FROM pr_fixes; DELETE FROM pr_origins;")).unwrap();
    let rules = [rule];
    let repository = cpython();
    {
        let mut keeper = Keeper {
            rules: &rules,
            ..Keeper::new(&connection, &repository).unwrap()
        };
        for facts in batches.iter().flatten() {
            keeper.rewrite(facts).unwrap();
        }
    }
    kept_rows(&connection)
}

/// The rows the store keeps, one line for each row.
fn kept_rows(connection: &Connection) -> Vec<String> {
    let mut rows = Vec::new();
    let fixes = "SELECT number, issue, under FROM pr_fixes ORDER BY number, issue, under";
    let mut statement = connection.prepare(fixes).unwrap();
    let mut found = statement.raw_query();
    while let Some(row) = found.next().unwrap() {
        let (number, issue, under): (u32, u32, String) = (
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
        );
        rows.push(format!("PR {number} fixes {issue} under {under}"));
    }
    let origins = "SELECT number, found_in, origin FROM pr_origins ORDER BY number, found_in";
    let mut statement = connection.prepare(origins).unwrap();
    let mut found = statement.raw_query();
    while let Some(row) = found.next().unwrap() {
        let (number, found_in, origin): (u32, String, u32) = (
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
        );
        rows.push(format!("PR {number} comes from {origin} by {found_in}"));
    }
    rows
}

#[test]
fn each_released_rule_keeps_the_rows_it_kept_for_its_sample_prs() {
    for rule in LinkRule::SET {
        let (batches, rows) = released(rule);
        assert_eq!(
            kept_by(rule, &batches),
            rows,
            "{rule:?} kept other rows: a released rule never changes what it gives, so make the change a \
             new LinkRule in its place in LinkRule::SET"
        );
    }
}

#[test]
fn the_set_keeps_the_rows_each_of_its_rules_keeps_alone() {
    let batches: Vec<Vec<PullRequestFacts>> = (LinkRule::SET.into_iter())
        .flat_map(|rule| released(rule).0)
        .collect();
    let alone: BTreeSet<String> = (LinkRule::SET.into_iter())
        .flat_map(|rule| kept_by(rule, &batches))
        .collect();
    let together: BTreeSet<String> = kept_rows(&saved(&batches)).into_iter().collect();
    assert_eq!(together, alone);
}

#[test]
fn the_version_of_the_rules_names_their_set() {
    assert_eq!(
        (RULES_VERSION, LinkRule::SET),
        (
            1,
            [
                LinkRule::ClosingReferences,
                LinkRule::DescriptionKeywords,
                LinkRule::CpythonTitleIssues,
                LinkRule::CpythonTitleOrigin,
                LinkRule::CherryPickLine,
            ]
        ),
        "raise RULES_VERSION with a change to LinkRule::SET, so the store works out every PR's links again"
    );
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
        before: at_second(100),
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
            updated_at: at_second(seconds),
            ..pr(number, "Fix it")
        })
        .collect();
    save(&mut connection, &prs);
    let numbers = |updates: Updates| -> Vec<u32> {
        let read = updated(&connection, &cpython(), updates).unwrap();
        read.into_iter().map(|(number, _)| number).collect()
    };

    let before = |seconds: i64, at_least: u32| Updates::Before {
        before: at_second(seconds),
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
    assert_eq!(numbers(Updates::Since(at_second(40))), [1, 3, 2]);
}
