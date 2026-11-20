use std::sync::mpsc::{self, Receiver};

use super::*;
use crate::facts::pull_requests::PullRequestFacts;
use crate::status::testing::quiet_board;
use crate::store::testing::{kept_fixes, refuse_new_links, store_with_old_links};
use crate::testing::{cpython, gcmon, pr_facts};

/// A board that passes on the Repository of each Change, with whether it named no item.
fn repositories_board() -> (StatusBoard, Receiver<(RepositoryName, bool)>) {
    let (heard, changes) = mpsc::channel();
    let board = quiet_board().with_changes(Box::new(move |change: &Change| {
        let named_none = change.items.is_empty();
        heard.send((change.repository.clone(), named_none)).unwrap();
    }));
    (board, changes)
}

#[test]
fn a_store_whose_links_the_set_worked_out_owes_no_refill() {
    let store = store_with_old_links(&[cpython()]);
    store.keep_link_rules().unwrap();

    assert!(!LinkRefill::owed_by(&store).is_due());
}

#[test]
fn a_store_that_cant_say_whether_its_links_are_stale_owes_no_refill_this_launch() {
    let store = store_with_old_links(&[cpython()]);
    let (lossy, _calls) = store.failing_at_call(Some(0));

    assert!(!LinkRefill::owed_by(&lossy).is_due());
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
}

#[test]
fn each_step_refills_a_repository_the_one_the_window_opened_first_and_the_last_saves_the_set() {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    let (mut board, changes) = repositories_board();
    let mut refill = LinkRefill::owed_by(&store);
    let opened = RepositoryName::parse("Some-Maintainer/GCMON").unwrap();
    refill.put_first(&opened);

    refill.step(&mut board, &store);

    assert_eq!(changes.try_iter().collect::<Vec<_>>(), [(gcmon(), true)]);
    assert!(refill.is_due());
    assert_eq!(store.stale_links().unwrap().map(|left| left.len()), Some(2));

    refill.step(&mut board, &store);

    assert_eq!(changes.try_iter().collect::<Vec<_>>(), [(cpython(), true)]);
    assert!(!refill.is_due());
    assert_eq!(store.stale_links().unwrap(), None);
    let fixed = |repository: RepositoryName| (repository.to_string(), 10, 5);
    assert_eq!(kept_fixes(&store), [fixed(cpython()), fixed(gcmon())]);
}

#[test]
fn each_batch_is_a_step_and_a_repository_keeps_its_place_when_another_goes_first() {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    let (mut board, changes) = repositories_board();
    let mut refill = LinkRefill {
        batch: NonZeroU32::MIN,
        ..LinkRefill::owed_by(&store)
    };
    refill.put_first(&cpython());

    // PR 10 of CPython, then the window opens gcmon, then the batch after PR 10, which finds none.
    refill.step(&mut board, &store);
    refill.put_first(&gcmon());
    for _ in 0..2 {
        refill.step(&mut board, &store);
    }
    assert_eq!(changes.try_iter().collect::<Vec<_>>(), [(gcmon(), true)]);
    let place = |left: &Left| (left.repository.clone(), left.from);
    let left: Vec<_> = refill.left.iter().flatten().map(place).collect();
    assert_eq!(left, [(cpython(), 11)]);

    refill.step(&mut board, &store);

    assert_eq!(changes.try_iter().collect::<Vec<_>>(), [(cpython(), true)]);
    assert!(!refill.is_due());
    let fixed = |repository: RepositoryName| (repository.to_string(), 10, 5);
    assert_eq!(kept_fixes(&store), [fixed(cpython()), fixed(gcmon())]);
}

#[test]
fn a_batch_that_fails_after_its_repositorys_first_tells_the_window_of_what_the_first_saved() {
    let store = store_with_old_links(&[cpython()]);
    let fixing_7 = PullRequestFacts {
        title: "gh-7: Fix it".to_owned(),
        ..pr_facts(11)
    };
    store
        .save_pull_requests(&cpython(), vec![fixing_7])
        .unwrap();
    let (mut board, changes) = repositories_board();
    let mut refill = LinkRefill {
        batch: NonZeroU32::MIN,
        ..LinkRefill::owed_by(&store)
    };
    refill.step(&mut board, &store);
    refuse_new_links(&store);

    refill.step(&mut board, &store);

    assert_eq!(changes.try_iter().collect::<Vec<_>>(), [(cpython(), true)]);
    assert!(!refill.is_due());
    let python = cpython().to_string();
    assert_eq!(
        kept_fixes(&store),
        [(python.clone(), 10, 5), (python, 11, 7)]
    );
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
}

#[test]
fn a_repository_the_refill_doesnt_hold_leaves_its_order_as_it_was() {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    let mut refill = LinkRefill::owed_by(&store);
    let order = refill.left.clone();

    refill.put_first(&RepositoryName::parse("other/repo").unwrap());

    assert_eq!(refill.left, order);
}

#[test]
fn a_repository_opened_once_the_refill_ended_finds_nothing_to_refill() {
    let store = store_with_old_links(&[cpython()]);
    store.keep_link_rules().unwrap();
    let mut refill = LinkRefill::owed_by(&store);

    refill.put_first(&cpython());
    refill.step(&mut quiet_board(), &store);

    assert!(!refill.is_due());
}

#[test]
fn a_refill_that_fails_ends_and_leaves_the_kept_links_with_their_version_for_the_next_launch() {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    refuse_new_links(&store);
    let (mut board, changes) = repositories_board();
    let mut refill = LinkRefill::owed_by(&store);

    refill.step(&mut board, &store);

    assert!(!refill.is_due());
    assert_eq!(changes.try_iter().count(), 0);
    let old = |repository: RepositoryName| (repository.to_string(), 10, 6);
    assert_eq!(kept_fixes(&store), [old(cpython()), old(gcmon())]);
    assert_eq!(store.stale_links().unwrap().map(|left| left.len()), Some(2));
}

#[test]
fn a_set_the_store_cant_save_leaves_the_refilled_links_for_the_next_launch_to_refill_again() {
    let store = store_with_old_links(&[cpython()]);
    let mut refill = LinkRefill::owed_by(&store);
    // The step's calls go: the refill, then the set's names, which go missing.
    let (lossy, _calls) = store.failing_at_call(Some(1));

    refill.step(&mut quiet_board(), &lossy);

    assert!(!refill.is_due());
    assert_eq!(kept_fixes(&store), [(cpython().to_string(), 10, 5)]);
    assert_eq!(store.stale_links().unwrap(), Some(vec![cpython()]));
}

#[test]
fn a_store_whose_stale_links_name_no_repository_saves_the_set_in_one_step() {
    let store = store_with_old_links(&[]);
    let mut refill = LinkRefill::owed_by(&store);
    assert!(refill.is_due());

    refill.step(&mut quiet_board(), &store);

    assert!(!refill.is_due());
    assert_eq!(store.stale_links().unwrap(), None);
}
