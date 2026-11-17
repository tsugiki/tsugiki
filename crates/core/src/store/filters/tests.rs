use super::*;
use crate::error::ErrorKind;
use crate::store::testing::OpenStore;
use crate::testing::cpython;

fn query(text: &str) -> FilterQuery {
    text.to_owned().try_into().unwrap()
}

fn found(numbers: &[u32]) -> FilterResults {
    FilterResults {
        numbers: numbers.iter().copied().collect(),
        problem: None,
    }
}

#[test]
fn a_new_search_replaces_what_the_last_one_found() {
    let db = OpenStore::new("filter-replace");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[1, 2]))
        .unwrap();
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[2, 3]))
        .unwrap();
    db.store
        .save_filter_results(&cpython(), &query("b"), found(&[9]))
        .unwrap();

    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(read[&query("a")], found(&[2, 3]));
    assert_eq!(read[&query("b")], found(&[9]));
}

#[test]
fn each_repository_has_its_own_results_whatever_the_case_of_its_name() {
    let db = OpenStore::new("filter-repositories");
    let other = RepositoryName::parse("python/peps").unwrap();
    let shouted = RepositoryName::parse("Python/CPython").unwrap();
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[1]))
        .unwrap();
    db.store
        .save_filter_results(&other, &query("a"), found(&[2]))
        .unwrap();

    let read = db.reader.work_facts(&shouted).unwrap().filters;
    db.close();

    assert_eq!(read[&query("a")], found(&[1]));
}

#[test]
fn a_problem_leaves_what_the_last_search_found_until_a_search_succeeds() {
    let db = OpenStore::new("filter-problem");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[1]))
        .unwrap();
    db.store
        .save_filter_problem(&cpython(), &query("a"), "It failed")
        .unwrap();
    db.store
        .save_filter_problem(&cpython(), &query("a"), "It failed again")
        .unwrap();
    let during = db.reader.work_facts(&cpython()).unwrap().filters;
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[2]))
        .unwrap();
    let after = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(
        during[&query("a")],
        FilterResults {
            numbers: BTreeSet::from([1]),
            problem: Some(FilterProblem::Refused("It failed again".to_owned())),
        }
    );
    assert_eq!(after[&query("a")], found(&[2]));
}

#[test]
fn results_can_come_with_a_problem() {
    let db = OpenStore::new("filter-cut-off");
    let cut_off = FilterResults {
        numbers: BTreeSet::from([1]),
        problem: Some(FilterProblem::Refused("It was cut off".to_owned())),
    };
    db.store
        .save_filter_results(&cpython(), &query("a"), cut_off.clone())
        .unwrap();

    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(read[&query("a")], cut_off);
}

#[test]
fn a_search_no_filter_holds_any_more_goes_with_its_problem() {
    let db = OpenStore::new("filter-edited");
    db.store
        .save_filter_results(&cpython(), &query("old"), found(&[1]))
        .unwrap();
    db.store
        .save_filter_problem(&cpython(), &query("broken"), "It failed")
        .unwrap();
    db.store
        .save_filter_results(&cpython(), &query("kept"), found(&[2]))
        .unwrap();

    db.store
        .keep_only_filters(&cpython(), vec![query("kept")])
        .unwrap();
    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(read, BTreeMap::from([(query("kept"), found(&[2]))]));
}

#[test]
fn a_search_that_found_nothing_counts_as_searched() {
    let db = OpenStore::new("filter-found-nothing");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[]))
        .unwrap();

    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(read, BTreeMap::from([(query("a"), found(&[]))]));
}

#[test]
fn a_search_the_user_edited_away_stops_counting_as_searched() {
    let db = OpenStore::new("filter-edited-empty");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[]))
        .unwrap();

    db.store.keep_only_filters(&cpython(), Vec::new()).unwrap();
    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(read, BTreeMap::new());
}

fn narrowed(changed: &[u32], found: &[u32]) -> NarrowedResults {
    NarrowedResults {
        changed: changed.iter().copied().collect(),
        found: found.iter().copied().collect(),
        cut_off: false,
    }
}

#[test]
fn a_narrowed_search_keeps_what_didnt_change_and_says_what_moved() {
    let db = OpenStore::new("filter-merge");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[1, 2, 3]))
        .unwrap();

    // 1 stopped matching, 2 still matches, 3 didn't change, 4 and 5 joined.
    let moved = db
        .store
        .merge_filter_results(&cpython(), &query("a"), narrowed(&[1, 2, 4, 9], &[2, 4, 5]))
        .unwrap();
    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(moved, BTreeSet::from([1, 4, 5]));
    assert_eq!(read[&query("a")], found(&[2, 3, 4, 5]));
}

#[test]
fn a_narrowed_search_keeps_a_problem_it_has_nothing_to_say_about() {
    let db = OpenStore::new("filter-merge-problem");
    let cut_off_before = FilterResults {
        numbers: BTreeSet::from([1]),
        problem: Some(FilterProblem::Refused("Cut off".to_owned())),
    };
    db.store
        .save_filter_results(&cpython(), &query("kept"), cut_off_before)
        .unwrap();

    let quiet = db
        .store
        .merge_filter_results(&cpython(), &query("kept"), narrowed(&[], &[]))
        .unwrap();
    // A search GitHub cut off may have missed 1, so 1 stays.
    let was_there = found(&[1]);
    db.store
        .save_filter_results(&cpython(), &query("new"), was_there)
        .unwrap();
    let cut_off = NarrowedResults {
        cut_off: true,
        ..narrowed(&[1], &[2])
    };
    let joined = db
        .store
        .merge_filter_results(&cpython(), &query("new"), cut_off)
        .unwrap();
    let read = db.reader.work_facts(&cpython()).unwrap().filters;
    db.close();

    assert_eq!(quiet, BTreeSet::new());
    assert_eq!(joined, BTreeSet::from([2]));
    assert_eq!(
        read[&query("kept")]
            .problem
            .as_ref()
            .map(FilterProblem::words),
        Some("Cut off")
    );
    assert_eq!(read[&query("new")].problem, Some(FilterProblem::CutOff));
    assert_eq!(read[&query("new")].numbers, BTreeSet::from([1, 2]));
}

#[test]
fn a_stored_search_tsugiki_cant_understand_is_a_read_error() {
    let db = OpenStore::new("filter-bad-row");
    db.store
        .save_filter_results(&cpython(), &query("a"), found(&[1]))
        .unwrap();
    let _ = db
        .store
        .call(|connection| connection.execute("UPDATE filter_matches SET number = 'one'", []))
        .unwrap()
        .unwrap();

    let refused = db.reader.work_facts(&cpython()).unwrap_err();
    db.close();

    assert_eq!(refused.kind, ErrorKind::Internal);
}
