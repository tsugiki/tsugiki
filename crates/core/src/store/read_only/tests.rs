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

#[test]
fn a_mark_and_its_clearing_each_say_whether_they_changed_it() {
    let db = OpenStore::new("read-only-marks");
    let first = db.store.mark_read_only(GH, &gcmon()).unwrap();
    let again = db.store.mark_read_only(GH, &gcmon()).unwrap();
    assert_eq!(db.read_only_marks(), (false, true));
    let cleared = db.store.clear_read_only(&gcmon()).unwrap();
    let unmarked = db.store.clear_read_only(&cpython()).unwrap();
    assert_eq!(db.read_only_marks(), (false, false));
    db.close();

    assert_eq!((first, again), (true, false));
    assert_eq!((cleared, unmarked), (true, false));
}

#[test]
fn another_token_forgets_the_marks_and_says_where_they_were() {
    let db = OpenStore::new("read-only-another-token");
    let first = fine_grained(2_000_000_000);
    assert!(db.store.mark_read_only(first, &cpython()).unwrap());
    let same = db.store.forget_read_only_of_other_tokens(first).unwrap();
    // The same kind with another expiry is another token.
    let renewed = fine_grained(2_100_000_000);
    let forgot = db.store.forget_read_only_of_other_tokens(renewed).unwrap();
    let kept = db.read_only_marks();

    assert!(db.store.mark_read_only(renewed, &gcmon()).unwrap());
    let same_kind = db
        .store
        .forget_read_only_of_other_kinds(TokenKind::FineGrained)
        .unwrap();
    let other_kind = db
        .store
        .forget_read_only_of_other_kinds(TokenKind::Classic)
        .unwrap();
    assert_eq!(db.read_only_marks(), (false, false));
    db.close();

    assert_eq!(same, []);
    assert_eq!(forgot, [cpython()]);
    assert_eq!(kept, (false, false));
    assert_eq!(same_kind, []);
    assert_eq!(other_kind, [gcmon()]);
}

#[test]
fn the_marks_of_repositories_no_longer_listed_go() {
    let db = OpenStore::new("read-only-unlisted");
    assert!(db.store.mark_read_only(GH, &cpython()).unwrap());
    assert!(db.store.mark_read_only(GH, &gcmon()).unwrap());
    let shouted = RepositoryName::parse("Python/CPython").unwrap();

    let forgot = db
        .store
        .forget_read_only_of_unlisted(vec![shouted])
        .unwrap();

    assert_eq!(forgot, [gcmon()]);
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}

#[test]
fn a_mark_survives_a_restart() {
    let (path, folder) = temp_database("read-only-restart");
    let store = Store::open(&path).unwrap();
    assert!(store.mark_read_only(GH, &cpython()).unwrap());
    store.close();

    let db = OpenStore::of(path, folder);
    assert_eq!(db.read_only_marks(), (true, false));
    db.close();
}
