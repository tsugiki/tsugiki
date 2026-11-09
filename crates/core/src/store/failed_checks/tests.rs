use time::{Duration, OffsetDateTime};

use crate::github::pull_requests::{CiState, PrState, PullRequestFacts};
use crate::login::Login;
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
        base: None,
        head: None,
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
