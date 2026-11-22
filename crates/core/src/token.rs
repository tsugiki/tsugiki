//! Reads the GitHub token through `tsugiki-token`, the crate that holds it (ADR 0019). The token never
//! reaches the webview (ADR 0005), and nothing in the core can read its string.

use std::io;

use tsugiki_token::{GH_TIMEOUT, NoToken};
pub use tsugiki_token::{GhOutput, Token, TokenFrom, TokenKind, WriteScope};

use crate::error::{Error, ErrorKind};
use crate::logfile;

/// What Tsugiki does next, and the way past `gh`, which a restart takes, since Tsugiki reads its own
/// environment, which a later change in the user's shell doesn't reach.
const ASKS_AGAIN: &str =
    "Tsugiki asks again each minute, and at once when you click an open Repository's sync status.";
const GH_TOKEN_INSTEAD: &str = "To skip `gh`, set `GH_TOKEN` to a token and restart Tsugiki.";

/// Takes the token from `GH_TOKEN` or asks `gh` for it, as [`TokenReads::read`] says, for a caller that
/// reads it once.
pub fn read_token(from: TokenFrom) -> Result<Token, Error> {
    TokenReads::default().read(from)
}

/// Reads of the token, one after another, which remember why the last one found none, so a read that
/// fails the same way each minute logs its cause once. A caller that reads more than once keeps one.
#[derive(Debug, Default)]
pub struct TokenReads {
    /// The cause of the last read's failure, when it found no token.
    last_failure: Option<Failure>,
}

/// The cause of a read that found no token, as far as telling one failure from the next goes. A `gh` the
/// system couldn't run is told apart by the system's reason, which [`NoToken`] holds as an [`io::Error`]
/// that can't be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    CantRun {
        kind: io::ErrorKind,
        os_error: Option<i32>,
    },
    TimedOut,
    Failed,
    PrintedNothing,
}

impl Failure {
    fn of(no_token: &NoToken) -> Self {
        match no_token {
            NoToken::GhMissing(cause) => Self::CantRun {
                kind: cause.kind(),
                os_error: cause.raw_os_error(),
            },
            NoToken::GhTimedOut => Self::TimedOut,
            NoToken::GhFailed => Self::Failed,
            NoToken::GhPrintedNothing => Self::PrintedNothing,
        }
    }
}

impl TokenReads {
    /// Takes the token from `GH_TOKEN` or asks `gh` for it, and hands the logger a copy to hide. Without
    /// one, the log says why, unless the last read failed the same way, and the user reads what happened,
    /// what Tsugiki does next and what they can do.
    pub fn read(&mut self, from: TokenFrom) -> Result<Token, Error> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the one caller clippy.toml allows, so every token is hidden in the log"
        )]
        let token = Token::find(from);
        match token {
            Ok(token) => {
                self.last_failure = None;
                logfile::hide(token.clone());
                Ok(token)
            }
            Err(no_token) => {
                let failure = Failure::of(&no_token);
                let (warning, message) = explain(no_token);
                if self.last_failure != Some(failure) {
                    log::warn!("{warning}");
                }
                self.last_failure = Some(failure);
                Err(Error::new(ErrorKind::Unauthorized, &message))
            }
        }
    }
}

/// The log's warning for a read that found no token, and the status bar's words for it.
fn explain(no_token: NoToken) -> (String, String) {
    match no_token {
        NoToken::GhMissing(cause) => (format!("Tsugiki can't run gh: {cause}"), sign_in()),
        NoToken::GhTimedOut => {
            let seconds = GH_TIMEOUT.as_secs();
            let message = format!(
                "Tsugiki asked `gh auth token` for your token, and `gh` didn't answer within {seconds} \
                 s. A keychain or credential manager waiting on a prompt is a common cause. {ASKS_AGAIN} \
                 {GH_TOKEN_INSTEAD}"
            );
            (
                format!("gh didn't answer within {seconds} s, so Tsugiki stopped it"),
                message,
            )
        }
        NoToken::GhFailed => ("gh failed when asked for its token".to_owned(), sign_in()),
        NoToken::GhPrintedNothing => (
            "gh printed no token when asked for its token".to_owned(),
            sign_in(),
        ),
    }
}

/// The words for a `gh` that isn't installed or isn't signed in.
fn sign_in() -> String {
    format!(
        "Tsugiki reads your token from the GitHub CLI, and found none. Install `gh` or run `gh auth \
         login`. {ASKS_AGAIN} {GH_TOKEN_INSTEAD}"
    )
}

#[cfg(test)]
mod tests;
