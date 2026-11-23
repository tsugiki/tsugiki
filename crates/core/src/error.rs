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
    /// A read of the token found none, so Tsugiki asks GitHub nothing until a read finds one. The status bar
    /// offers Try again beside it, which reads the token at once.
    NoToken,
    /// GitHub refused the token.
    Unauthorized,
    WriteRefused,
    RateLimited {
        /// The time Tsugiki can ask GitHub again, by this computer's clock, in seconds since the Unix epoch,
        /// if known.
        resets_at: Option<u32>,
    },
    NotFound,
    Conflict,
    /// GitHub took the request and answered with an error about it, or `tsugiki-token` refused to send it
    /// (ADR 0025). Asking again fails the same way. A failed status is `Internal`.
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
            | Self::NoToken
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
