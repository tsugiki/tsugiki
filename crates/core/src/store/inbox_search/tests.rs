use super::*;
use crate::facts::inbox_search::{MAX_RESULTS, RowKind, SearchHit};
use crate::store::testing::OpenStore;
use crate::testing::{cpython, moment};

/// A result GitHub listed, updated at `minute` past ten on the fourth.
fn hit(number: u32, kind: RowKind, minute: u8) -> SearchHit {
    SearchHit {
        number,
        kind,
        updated_at: moment(&format!("2026-11-04T10:{minute:02}:00Z")),
    }
}

/// GitHub's answer listing `hits`, of `total` results, and no more to read.
fn answer(hits: Vec<SearchHit>, total: u32) -> InboxSearch {
    InboxSearch {
        hits,
        total,
        cut_off: false,
    }
}

/// GitHub's answer that stopped at 1,000 results, of `total`.
fn cut_off(hits: Vec<SearchHit>, total: u32) -> InboxSearch {
    InboxSearch {
        hits,
        total,
        cut_off: true,
    }
}

fn read(db: &OpenStore, query: &str) -> Option<InboxSearch> {
    let query = query.to_owned();
    db.reader
        .read_snapshot(&cpython(), move |snapshot| snapshot.inbox_search(&query))
        .unwrap()
}

#[test]
fn an_answer_reads_back_in_github_s_order_with_its_total() {
    let db = OpenStore::new("inbox-search-order");
    let wrote = answer(
        vec![
            hit(30, RowKind::PullRequest, 3),
            hit(20, RowKind::Issue, 2),
            hit(10, RowKind::PullRequest, 1),
        ],
        7,
    );
    db.store
        .save_inbox_search(&cpython(), "fix parser", wrote.clone())
        .unwrap();

    let read = read(&db, "fix parser");
    db.close();

    assert_eq!(read, Some(wrote));
}

#[test]
fn a_new_query_drops_the_answer_to_the_old_one() {
    let db = OpenStore::new("inbox-search-replace");
    db.store
        .save_inbox_search(
            &cpython(),
            "old",
            answer(vec![hit(1, RowKind::Issue, 1)], 1),
        )
        .unwrap();
    let newer = answer(vec![hit(2, RowKind::PullRequest, 2)], 1);
    db.store
        .save_inbox_search(&cpython(), "new", newer.clone())
        .unwrap();

    let old = read(&db, "old");
    let new = read(&db, "new");
    db.close();

    assert_eq!(old, None);
    assert_eq!(new, Some(newer));
}

#[test]
fn a_search_past_github_s_cap_reads_back_as_cut_off() {
    let db = OpenStore::new("inbox-search-cut-off");
    let hits: Vec<SearchHit> = (0..MAX_RESULTS)
        .map(|number| hit(u32::try_from(number).unwrap(), RowKind::Issue, 1))
        .collect();
    let cut_off = cut_off(hits, 1712);
    db.store
        .save_inbox_search(&cpython(), "is:issue", cut_off.clone())
        .unwrap();

    let read = read(&db, "is:issue");
    db.close();

    assert_eq!(read, Some(cut_off));
    assert_eq!(
        read.unwrap().hits.len(),
        MAX_RESULTS,
        "GitHub lists 1,000, and Tsugiki reads them all"
    );
}

#[test]
fn a_repository_with_no_answer_reads_none() {
    let db = OpenStore::new("inbox-search-none");

    let read = read(&db, "fix");
    db.close();

    assert_eq!(read, None);
}
