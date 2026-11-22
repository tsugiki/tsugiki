//! Reads the GitHub token through `tsugiki-token`, the crate that holds it (ADR 0019). The token never
//! reaches the webview (ADR 0005), and nothing in the core can read its string.

use tsugiki_token::{GH_TIMEOUT, NoToken};
pub use tsugiki_token::{GhOutput, Token, TokenFrom, TokenKind, WriteScope};

use crate::error::{Error, ErrorKind};
use crate::logfile;

/// What Tsugiki does next, and the way past `gh`, which a restart takes, since Tsugiki reads its own
/// environment, which a later change in the user's shell doesn't reach.
const ASKS_AGAIN: &str =
    "Tsugiki asks again each minute, and at once when you click an open Repository's sync status.";
const GH_TOKEN_INSTEAD: &str = "To skip `gh`, set `GH_TOKEN` to a token and restart Tsugiki.";

/// Takes the token from `GH_TOKEN` or asks `gh` for it, and hands the logger a copy to hide. Without one,
/// the log says why, and the user reads what happened, what Tsugiki does next and what they can do.
pub fn read_token(from: TokenFrom) -> Result<Token, Error> {
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
                    sign_in()
                }
                NoToken::GhTimedOut => {
                    let seconds = GH_TIMEOUT.as_secs();
                    log::warn!("gh didn't answer within {seconds} s, so Tsugiki stopped it");
                    format!(
                        "Tsugiki asked `gh auth token` for your token, and `gh` didn't answer within \
                         {seconds} s. A keychain or credential manager waiting on a prompt is a common \
                         cause. {ASKS_AGAIN} {GH_TOKEN_INSTEAD}"
                    )
                }
                NoToken::GhFailed => {
                    log::warn!("gh failed when asked for its token");
                    sign_in()
                }
                NoToken::GhPrintedNothing => {
                    log::warn!("gh printed no token when asked for its token");
                    sign_in()
                }
            };
            Err(Error::new(ErrorKind::Unauthorized, &message))
        }
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
