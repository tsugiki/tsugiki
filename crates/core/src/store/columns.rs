//! The conversions between Tsugiki's types and the store's columns, in one place, so queries work in those
//! types.

use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use time::OffsetDateTime;

use crate::filters::FilterQuery;
use crate::github::pull_requests::{CiState, PrState, ReviewState};
use crate::github::refresh::Etag;
use crate::github::{CommitSha, Login};
use crate::repository::RepositoryName;

/// A time in a column, as seconds since the Unix epoch.
pub(super) struct UnixTime(pub(super) OffsetDateTime);

impl FromSql for UnixTime {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        OffsetDateTime::from_unix_timestamp(value.as_i64()?)
            .map(Self)
            .map_err(|cause| FromSqlError::Other(Box::new(cause)))
    }
}

impl ToSql for RepositoryName {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl ToSql for Login {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for Login {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value.as_str().map(Self::new)
    }
}

impl ToSql for FilterQuery {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for FilterQuery {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Self::try_from(value.as_str()?.to_owned())
            .map_err(|problem| FromSqlError::Other(problem.into()))
    }
}

impl ToSql for Etag {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for Etag {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value.as_str().map(Self::new)
    }
}

impl ToSql for CommitSha {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for CommitSha {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Self::parse(text)
            .ok_or_else(|| FromSqlError::Other(format!("{text} isn't a commit's SHA").into()))
    }
}

/// A fieldless enum of GitHub's as text, spelled as GitHub spells it. The `match` makes a new variant name
/// its text, and a test pins each text, because existing databases hold them.
macro_rules! text_column {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(match self { $(Self::$variant => $text),+ }))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                match value.as_str()? {
                    $($text => Ok(Self::$variant),)+
                    other => Err(FromSqlError::Other(
                        format!("{} has no {other}", stringify!($name)).into(),
                    )),
                }
            }
        }

        #[cfg(test)]
        impl $name {
            const ALL: &[Self] = &[$(Self::$variant),+];
        }
    };
}

text_column!(PrState {
    Open => "OPEN",
    Closed => "CLOSED",
    Merged => "MERGED",
});

text_column!(CiState {
    Expected => "EXPECTED",
    Error => "ERROR",
    Failure => "FAILURE",
    Pending => "PENDING",
    Success => "SUCCESS",
});

text_column!(ReviewState {
    Pending => "PENDING",
    Commented => "COMMENTED",
    Approved => "APPROVED",
    ChangesRequested => "CHANGES_REQUESTED",
    Dismissed => "DISMISSED",
});

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, named_params};
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
    fn each_state_keeps_the_text_earlier_versions_stored() {
        assert_eq!(stored_texts(PrState::ALL), ["OPEN", "CLOSED", "MERGED"]);
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
}
