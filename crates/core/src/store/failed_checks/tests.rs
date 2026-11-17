use time::OffsetDateTime;

use crate::facts::pull_requests::{CiState, PrState, PullRequestFacts};
use crate::login::Login;
use crate::store::Store;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, hours_ago, me, pr_facts};

fn failing(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(me()),
        ci: Some(CiState::Failure),
        updated_at: hours_ago(1),
        ..pr_facts(number)
    }
}

fn unread(store: &Store) -> Vec<u32> {
    store
        .pull_requests_with_unread_checks(&cpython(), &me())
        .unwrap()
}

#[test]
fn only_the_users_open_prs_whose_ci_failed_are_worth_asking_about() {
    let db = OpenStore::new("failed-checks-which");
    db.store
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

    assert_eq!(unread(&db.store), [1, 5]);
    db.close();
}

#[test]
fn asking_once_is_enough_and_the_names_replace_the_last_ones() {
    let db = OpenStore::new("failed-checks-once");
    db.store
        .save_pull_requests(&cpython(), vec![failing(1)])
        .unwrap();
    let first = vec!["Docs".to_owned(), "Tests".to_owned()];
    db.store
        .save_failed_checks(&cpython(), vec![(1, first)])
        .unwrap();
    assert_eq!(unread(&db.store), [0_u32; 0]);

    let then = vec!["Tests".to_owned()];
    db.store
        .save_failed_checks(&cpython(), vec![(1, then.clone())])
        .unwrap();
    let facts = db.reader.work_facts(&cpython()).unwrap();

    assert_eq!(facts.activity.failed_checks.get(&1), Some(&then));
    db.close();
}

#[test]
fn a_pr_that_moved_while_failing_is_worth_asking_about_again() {
    let db = OpenStore::new("failed-checks-moved");
    db.store
        .save_pull_requests(&cpython(), vec![failing(1)])
        .unwrap();
    db.store
        .save_failed_checks(&cpython(), vec![(1, vec!["Docs".to_owned()])])
        .unwrap();
    db.store
        .save_pull_requests(
            &cpython(),
            vec![PullRequestFacts {
                updated_at: OffsetDateTime::now_utc(),
                ..failing(1)
            }],
        )
        .unwrap();

    assert_eq!(unread(&db.store), [1]);
    db.close();
}

#[test]
fn a_later_failure_is_worth_asking_about_again() {
    let db = OpenStore::new("failed-checks-again");
    db.store
        .save_pull_requests(&cpython(), vec![failing(1)])
        .unwrap();
    db.store
        .save_failed_checks(&cpython(), vec![(1, vec!["Docs".to_owned()])])
        .unwrap();
    db.store
        .save_pull_requests(
            &cpython(),
            vec![PullRequestFacts {
                ci: Some(CiState::Pending),
                ..failing(1)
            }],
        )
        .unwrap();
    db.store
        .save_pull_requests(&cpython(), vec![failing(1)])
        .unwrap();

    assert_eq!(unread(&db.store), [1]);
    db.close();
}
