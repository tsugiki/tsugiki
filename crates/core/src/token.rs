//! Reads the GitHub token through `tsugiki-token`, the crate that holds it (ADR 0019). The token never
//! reaches the webview (ADR 0005), and nothing in the core can read its string.

use tsugiki_token::{GH_TIMEOUT, NoToken};
pub use tsugiki_token::{GhOutput, Token, TokenFrom};

use crate::error::{Error, ErrorKind};
use crate::logfile;

const SIGN_IN: &str = "Tsugiki reads your token from the GitHub CLI. Install `gh`, run \
                       `gh auth login`, and restart Tsugiki.";

/// The moment Tsugiki next asks for the token after a read finds none. The words for a `gh` that didn't
/// answer in time name it, since waiting for it is all the user can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskAgain {
    /// Only once Tsugiki restarts, as for the session's connection.
    AtRestart,
    /// On the next try of what asked, as for a clone, which goes back in line after a wait.
    OnRetry,
}

/// Takes the token from `GH_TOKEN` or asks `gh` for it, and hands the logger a copy to hide. Without one,
/// the log says why and the user reads what to do: sign in, or, when `gh` didn't answer in time, wait for
/// `again`.
pub fn read_token(from: TokenFrom, again: AskAgain) -> Result<Token, Error> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one caller clippy.toml allows, so every token is hidden in the log"
    )]
    let token = Token::find(from);
    match token {
        Ok(token) => {
            logfile::hide(token.clone());
            Ok(token)
        }
        Err(no_token) => {
            let message = match no_token {
                NoToken::GhMissing(cause) => {
                    log::warn!("Tsugiki can't run gh: {cause}");
                    SIGN_IN.to_owned()
                }
                NoToken::GhTimedOut => {
                    let seconds = GH_TIMEOUT.as_secs();
                    log::warn!("gh didn't answer within {seconds} s, so Tsugiki stopped it");
                    let next = match again {
                        AskAgain::AtRestart => "Restart Tsugiki to ask it again.",
                        AskAgain::OnRetry => "Tsugiki asks it again on the next try.",
                    };
                    format!(
                        "Tsugiki reads your token from the GitHub CLI, and `gh` didn't answer within \
                         {seconds} s. {next}"
                    )
                }
                NoToken::GhFailed => {
                    log::warn!("gh failed when asked for its token");
                    SIGN_IN.to_owned()
                }
                NoToken::GhPrintedNothing => {
                    log::warn!("gh printed no token when asked for its token");
                    SIGN_IN.to_owned()
                }
            };
            Err(Error::new(ErrorKind::Unauthorized, &message))
        }
    }
}

#[cfg(test)]
mod tests;
