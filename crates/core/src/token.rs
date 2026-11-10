//! Reads the GitHub token through `tsugiki-token`, the crate that holds it (ADR 0019). The token never
//! reaches the webview (ADR 0005), and nothing in the core can read its string.

use tsugiki_token::NoToken;
pub use tsugiki_token::{GhOutput, RunGh, Token};

use crate::error::{Error, ErrorKind};
use crate::logfile;

const SIGN_IN: &str = "Tsugiki reads your token from the GitHub CLI. Install `gh`, run \
                       `gh auth login`, and restart Tsugiki.";

/// Asks `gh` for its token, and hands the logger a copy to hide. Without one, the log says why and the
/// user reads how to sign in.
pub fn read_token(run: RunGh) -> Result<Token, Error> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one caller clippy.toml allows, so every token is hidden in the log"
    )]
    let token = Token::from_gh(run);
    match token {
        Ok(token) => {
            logfile::hide(token.clone());
            Ok(token)
        }
        Err(no_token) => {
            match no_token {
                NoToken::GhMissing(cause) => log::warn!("Tsugiki can't run gh: {cause}"),
                NoToken::GhFailed => log::warn!("gh failed when asked for its token"),
                NoToken::GhPrintedNothing => {
                    log::warn!("gh printed no token when asked for its token")
                }
            }
            Err(Error::new(ErrorKind::Unauthorized, SIGN_IN))
        }
    }
}

#[cfg(test)]
mod tests;
