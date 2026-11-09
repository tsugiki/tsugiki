//! The errors the UI can act on (ADR 0008).

use serde::Serialize;
use specta::Type;

/// What the UI can do something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorKind {
    Offline,
    /// GitHub's certificate isn't valid at this computer's date and time, so no request reaches GitHub.
    CertificateDates,
    Unauthorized,
    WriteRefused,
    RateLimited {
        /// When the limit resets, in seconds since the Unix epoch, if GitHub said.
        resets_at: Option<u32>,
    },
    NotFound,
    Conflict,
    /// GitHub took the request and answered with an error about it. A failed status is `Internal`.
    QueryRefused,
    Internal,
}

impl ErrorKind {
    /// True when no other request would fare better, so the trouble isn't about the one thing that was asked.
    pub fn speaks_for_all_of_github(&self) -> bool {
        match self {
            Self::Offline
            | Self::CertificateDates
            | Self::RateLimited { .. }
            | Self::Unauthorized => true,
            Self::WriteRefused
            | Self::NotFound
            | Self::Conflict
            | Self::QueryRefused
            | Self::Internal => false,
        }
    }
}

/// A failure in plain words. Details go only to a local log, with tokens scrubbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Error {
    #[serde(flatten)]
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests;
