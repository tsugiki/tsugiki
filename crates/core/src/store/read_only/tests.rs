use time::OffsetDateTime;

use super::*;
use crate::store::Store;
use crate::store::testing::{OpenStore, temp_database};
use crate::testing::{cpython, gcmon};

/// gh's token, which doesn't expire.
const GH: TokenSeen = TokenSeen {
    kind: TokenKind::OAuth,
    expires: None,
};

/// A fine-grained token that expires at `seconds` past the Unix epoch.
fn fine_grained(seconds: i64) -> TokenSeen {
    TokenSeen {
        kind: TokenKind::FineGrained,
        expires: Some(OffsetDateTime::from_unix_timestamp(seconds).unwrap()),
    }
}

fn both() -> Vec<RepositoryName> {
    vec![cpython(), gcmon()]
}

#[test]
fn the_scopes_mark_the_repositories_they_dont_reach_and_say_which_changed() {
    let db = OpenStore::new("read-only-marks");
    let judged = TokenJudged::CantWrite(vec![gcmon()]);
    let changed = db.store.judge_read_only(GH, both(), judged).unwrap();
    assert_eq!(changed, [gcmon()]);
    assert_eq!(db.read_only_marks(), (false, true));

    let judged = TokenJudged::CantWrite(vec![cpython(), gcmon()]);
    let changed = db.store.judge_read_only(GH, both(), judged).unwrap();
    assert_eq!(changed, [cpython()]);

    let changed = db
        .store
        .judge_read_only(GH, both(), TokenJudged::CantWrite(vec![]))
        .unwrap();
    assert_eq!(changed, both());
    assert_eq!(db.read_only_marks(), (false, false));
    db.close();
}

#[test]
fn silent_scopes_keep_the_token_s_own_marks() {
    let db = OpenStore::new("read-only-silent");
    let token = fine_grained(2_000_000_000);
    let marked = (db.store)
        .judge_read_only(token, both(), TokenJudged::CantWrite(vec![cpython()]))
        .unwrap();
    assert_eq!(marked, [cpython()]);
    let changed = db
        .store
        .judge_read_only(token, both(), TokenJudged::Silent)
        .unwrap();
    assert_eq!(changed, []);
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}

#[test]
fn another_token_clears_the_marks() {
    let db = OpenStore::new("read-only-another-token");
    let first = fine_grained(2_000_000_000);
    let marked = (db.store)
        .judge_read_only(first, both(), TokenJudged::CantWrite(vec![cpython()]))
        .unwrap();
    assert_eq!(marked, [cpython()]);
    // The same kind with another expiry is another token.
    let renewed = fine_grained(2_100_000_000);
    let changed = db
        .store
        .judge_read_only(renewed, both(), TokenJudged::Silent)
        .unwrap();
    assert_eq!(changed, [cpython()]);
    assert_eq!(db.read_only_marks(), (false, false));

    let marked = (db.store)
        .judge_read_only(renewed, both(), TokenJudged::CantWrite(vec![gcmon()]))
        .unwrap();
    assert_eq!(marked, [gcmon()]);
    let changed = db
        .store
        .judge_read_only(GH, both(), TokenJudged::Silent)
        .unwrap();
    assert_eq!(changed, [gcmon()]);
    assert_eq!(db.read_only_marks(), (false, false));
    db.close();
}

#[test]
fn a_mark_survives_a_restart() {
    let (path, folder) = temp_database("read-only-restart");
    let store = Store::open(&path).unwrap();
    let judged = TokenJudged::CantWrite(vec![cpython()]);
    assert_eq!(
        store.judge_read_only(GH, both(), judged).unwrap(),
        [cpython()]
    );
    store.close();

    let db = OpenStore::of(path, folder);
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}
