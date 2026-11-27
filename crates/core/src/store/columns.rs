//! The conversions between Tsugiki's types and the store's columns, in one place, so queries work in those
//! types.

use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use time::OffsetDateTime;

use crate::facts::CommitSha;
use crate::facts::inbox_search::RowKind;
use crate::facts::issue_content::{CommentId, HiddenReason, IssueEvent};
use crate::facts::issues::IssueState;
use crate::facts::labels::Label;
use crate::facts::patch_ids::PatchId;
use crate::facts::pull_request_content::{
    Anchor, Resolution, ReviewId, Side, Submission, ThreadCommentId,
};
use crate::facts::pull_requests::{CiState, NodeId, PrState, ReviewState};
use crate::filters::FilterQuery;
use crate::github::refresh::Etag;
use crate::login::Login;
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

/// A time in a column, as milliseconds since the Unix epoch, for an order that two moments in one second
/// mustn't tie in, as the Archived view's doesn't (#491).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UnixMillis(pub(super) OffsetDateTime);

impl ToSql for UnixMillis {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let Self(at) = self;
        let millis = at.unix_timestamp() * 1000 + i64::from(at.millisecond());
        Ok(ToSqlOutput::from(millis))
    }
}

impl FromSql for UnixMillis {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(value.as_i64()?) * 1_000_000)
            .map(Self)
            .map_err(|cause| FromSqlError::Other(Box::new(cause)))
    }
}

/// GitHub's ID of a node in a column, as text. Outside tests a `NodeId` comes only from parsing JSON, so the
/// store keeps the string it serializes to, and parses it back the same way. Neither way fails for an ID,
/// which is a string both ways, so each passes an error on through named functions, not closures that never
/// run.
pub(super) struct NodeColumn(pub(super) NodeId);

/// The cause a conversion error of rusqlite's carries.
type Cause = Box<dyn std::error::Error + Send + Sync>;

impl ToSql for NodeColumn {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        serde_json::to_value(&self.0)
            .and_then(serde_json::from_value::<String>)
            .map(ToSqlOutput::from)
            .map_err(Cause::from)
            .map_err(rusqlite::Error::ToSqlConversionFailure)
    }
}

impl FromSql for NodeColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = serde_json::Value::String(value.as_str()?.to_owned());
        serde_json::from_value(text)
            .map(Self)
            .map_err(Cause::from)
            .map_err(FromSqlError::Other)
    }
}

/// An Issue event in a column, as JSON: its kind carries what the event names, and only the Issue view reads
/// it (#1067).
pub(super) struct EventColumn(pub(super) IssueEvent);

impl ToSql for EventColumn {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        serde_json::to_string(&self.0)
            .map(ToSqlOutput::from)
            .map_err(Cause::from)
            .map_err(rusqlite::Error::ToSqlConversionFailure)
    }
}

impl FromSql for EventColumn {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        serde_json::from_str(value.as_str()?)
            .map(Self)
            .map_err(Cause::from)
            .map_err(FromSqlError::Other)
    }
}

/// Some Issue and PR numbers as one parameter: a JSON array, which a query reads with `json_each`, so one
/// statement asks for all of them. A caller that asks with the same numbers more than once builds it once.
pub struct Numbers(String);

impl Numbers {
    pub fn of(numbers: impl IntoIterator<Item = u32>) -> Self {
        let listed: Vec<String> = numbers
            .into_iter()
            .map(|number| number.to_string())
            .collect();
        Self(format!("[{}]", listed.join(",")))
    }
}

impl ToSql for Numbers {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

/// Which items' rows a read takes: every item's, or those of the items `Numbers` lists.
#[derive(Clone, Copy)]
pub(super) enum Items<'a> {
    All,
    Numbered(&'a Numbers),
}

impl<'a> Items<'a> {
    /// The condition on the number column `column` that keeps these items' rows, which follows the other
    /// conditions of a `WHERE`. It names `:numbers`, which `with` binds.
    pub(super) fn condition(self, column: &str) -> String {
        match self {
            Self::All => String::new(),
            Self::Numbered(_) => format!("AND {column} IN (SELECT value FROM json_each(:numbers))"),
        }
    }

    /// `asked`, with the parameter `condition` names, if it names one.
    pub(super) fn with<'p>(
        self,
        mut asked: Vec<(&'p str, &'p dyn ToSql)>,
    ) -> Vec<(&'p str, &'p dyn ToSql)>
    where
        'a: 'p,
    {
        if let Self::Numbered(numbers) = self {
            asked.push((":numbers", numbers));
        }
        asked
    }
}

impl ToSql for RepositoryName {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for RepositoryName {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Self::parse(value.as_str()?).map_err(|problem| FromSqlError::Other(problem.message.into()))
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

impl ToSql for Label {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for Label {
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

impl ToSql for PatchId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for PatchId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        Self::parse(text)
            .ok_or_else(|| FromSqlError::Other(format!("{text} isn't a patch-id").into()))
    }
}

/// One of GitHub's numbers for a comment or a Review, as SQLite's signed integer bit for bit, so even a
/// number past `i64::MAX` comes back as it went in.
macro_rules! number_column {
    ($($name:ident),+ $(,)?) => {$(
        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.get().cast_signed()))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                value.as_i64().map(|number| Self::new(number.cast_unsigned()))
            }
        }
    )+};
}

number_column!(CommentId, ReviewId, ThreadCommentId);

/// A fieldless enum of GitHub's as text, spelled as GitHub spells it. The `match` makes a new variant name
/// its text, and a test pins each text, because existing databases hold them.
macro_rules! text_column {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        text_column!($name { $(($variant) => $text),+ });
    };
    // A variant that holds a value names it in parentheses, one spelling for each value it can hold.
    ($name:ident { $(($($variant:tt)+) => $text:literal),+ $(,)? }) => {
        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::from(match self { $(Self::$($variant)+ => $text),+ }))
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                match value.as_str()? {
                    $($text => Ok(Self::$($variant)+),)+
                    other => Err(rusqlite::types::FromSqlError::Other(
                        format!("{} has no {other}", stringify!($name)).into(),
                    )),
                }
            }
        }

        #[cfg(test)]
        impl $name {
            const ALL: &[Self] = &[$(Self::$($variant)+),+];
        }
    };
}

pub(super) use text_column;

text_column!(PrState {
    Open => "OPEN",
    Closed => "CLOSED",
    Merged => "MERGED",
});

text_column!(IssueState {
    Open => "OPEN",
    Closed => "CLOSED",
    NotPlanned => "NOT_PLANNED",
    Duplicate => "DUPLICATE",
});

text_column!(RowKind {
    Issue => "issue",
    PullRequest => "pull_request",
});

text_column!(CiState {
    Expected => "EXPECTED",
    Error => "ERROR",
    Failure => "FAILURE",
    Pending => "PENDING",
    Success => "SUCCESS",
});

text_column!(HiddenReason {
    Abuse => "abuse",
    OffTopic => "off-topic",
    Outdated => "outdated",
    Resolved => "resolved",
    Duplicate => "duplicate",
    Spam => "spam",
    Unstated => "unstated",
});

text_column!(Resolution {
    Open => "open",
    Resolved => "resolved",
});

text_column!(Anchor {
    Current => "current",
    Outdated => "outdated",
});

text_column!(Side {
    Left => "LEFT",
    Right => "RIGHT",
});

text_column!(Submission {
    Submitted => "submitted",
    Pending => "pending",
});

text_column!(ReviewState {
    Pending => "PENDING",
    Commented => "COMMENTED",
    Approved => "APPROVED",
    ChangesRequested => "CHANGES_REQUESTED",
    Dismissed => "DISMISSED",
});

#[cfg(test)]
mod tests;
