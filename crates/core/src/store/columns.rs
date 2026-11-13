//! The conversions between Tsugiki's types and the store's columns, in one place, so queries work in those
//! types.

use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use time::OffsetDateTime;

use crate::filters::FilterQuery;
use crate::github::CommitSha;
use crate::github::issue_content::{CommentId, HiddenReason};
use crate::github::issues::IssueState;
use crate::github::labels::Label;
use crate::github::pull_request_content::{
    Anchor, Resolution, ReviewId, Side, Submission, ThreadCommentId,
};
use crate::github::pull_requests::{CiState, PrState, ReviewState};
use crate::github::refresh::Etag;
use crate::login::Login;
use crate::patch_ids::PatchId;
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

text_column!(IssueState {
    Open => "OPEN",
    Closed => "CLOSED",
    NotPlanned => "NOT_PLANNED",
    Duplicate => "DUPLICATE",
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
