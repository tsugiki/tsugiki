use super::*;
use crate::store::Connection;
use rusqlite::named_params;
use std::fmt::Debug;

/// The text `value` leaves in a column, and the value that reads back from there.
fn through_a_column<T: ToSql + FromSql>(value: &T) -> (String, T) {
    Connection::open_in_memory()
        .unwrap()
        .query_row(
            "SELECT :value AS value",
            named_params! {":value": value},
            |row| Ok((row.get("value")?, row.get("value")?)),
        )
        .unwrap()
}

/// The text each of `all` leaves in a column, once it has read back as itself.
fn stored_texts<T: ToSql + FromSql + Debug + PartialEq>(all: &[T]) -> Vec<String> {
    let through = |value| {
        let (text, read) = through_a_column(value);
        assert_eq!(&read, value);
        text
    };
    all.iter().map(through).collect()
}

#[test]
fn a_time_in_milliseconds_keeps_them_and_refuses_one_past_the_calendar() {
    let at = OffsetDateTime::from_unix_timestamp_nanos(1_795_000_000_123_456_789).unwrap();
    let connection = Connection::open_in_memory().unwrap();
    let (stored, read): (i64, UnixMillis) = connection
        .query_row(
            "SELECT :value AS value",
            named_params! {":value": UnixMillis(at)},
            |row| Ok((row.get("value")?, row.get("value")?)),
        )
        .unwrap();
    let past: rusqlite::Result<UnixMillis> =
        connection.query_row("SELECT 9223372036854775807 AS value", [], |row| {
            row.get("value")
        });

    assert_eq!(stored, 1_795_000_000_123);
    assert_eq!(
        read,
        UnixMillis(at.replace_nanosecond(123_000_000).unwrap())
    );
    assert!(past.is_err());
}

#[test]
fn each_mark_on_a_thread_keeps_its_text() {
    assert_eq!(stored_texts(Resolution::ALL), ["open", "resolved"]);
    assert_eq!(stored_texts(Anchor::ALL), ["current", "outdated"]);
    assert_eq!(stored_texts(Side::ALL), ["LEFT", "RIGHT"]);
    assert_eq!(stored_texts(Submission::ALL), ["submitted", "pending"]);
}

#[test]
fn each_reason_to_hide_a_comment_keeps_its_text() {
    assert_eq!(
        stored_texts(HiddenReason::ALL),
        [
            "abuse",
            "off-topic",
            "outdated",
            "resolved",
            "duplicate",
            "spam",
            "unstated"
        ]
    );
}

#[test]
fn each_state_keeps_the_text_earlier_versions_stored() {
    assert_eq!(stored_texts(PrState::ALL), ["OPEN", "CLOSED", "MERGED"]);
    assert_eq!(
        stored_texts(IssueState::ALL),
        ["OPEN", "CLOSED", "NOT_PLANNED", "DUPLICATE"]
    );
    assert_eq!(
        stored_texts(CiState::ALL),
        ["EXPECTED", "ERROR", "FAILURE", "PENDING", "SUCCESS"]
    );
    assert_eq!(
        stored_texts(ReviewState::ALL),
        [
            "PENDING",
            "COMMENTED",
            "APPROVED",
            "CHANGES_REQUESTED",
            "DISMISSED"
        ]
    );
}

#[test]
fn a_commit_sha_column_refuses_text_that_isnt_one() {
    let refused = Connection::open_in_memory().unwrap().query_row(
        "SELECT 'not a commit' AS value",
        [],
        |row| row.get::<_, CommitSha>("value"),
    );
    assert!(refused.is_err());
}

#[test]
fn a_patch_id_column_refuses_text_that_isnt_one() {
    let refused =
        Connection::open_in_memory()
            .unwrap()
            .query_row("SELECT 'abc1234' AS value", [], |row| {
                row.get::<_, crate::facts::patch_ids::PatchId>("value")
            });
    assert!(refused.is_err());
}

#[test]
fn a_filter_query_reads_back_as_itself_and_refuses_a_blank() {
    let query = FilterQuery::try_from("is:open".to_owned()).unwrap();
    assert_eq!(stored_texts(&[query]), ["is:open"]);
    let refused =
        Connection::open_in_memory()
            .unwrap()
            .query_row("SELECT '  ' AS value", [], |row| {
                row.get::<_, FilterQuery>("value")
            });
    assert!(refused.is_err());
}

#[test]
fn a_repository_goes_in_as_the_user_wrote_it() {
    let name = RepositoryName::parse("Python/CPython").unwrap();
    let found: String = Connection::open_in_memory()
        .unwrap()
        .query_row(
            "SELECT :name AS name",
            named_params! {":name": name},
            |row| row.get("name"),
        )
        .unwrap();
    assert_eq!(found, "Python/CPython");
}
