//! The GitHub token. It lives only in the core and never reaches the webview (ADR 0005).

use std::fmt;
use std::io;

use crate::error::{Error, ErrorKind};
use crate::logfile;

const SIGN_IN: &str = "Tsugiki reads your token from the GitHub CLI. Install `gh`, run \
                       `gh auth login`, and restart Tsugiki.";

/// What running `gh` printed.
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
}

/// Runs `gh` with the given arguments: the real program in the app, a stand-in in tests.
pub type RunGh = fn(&[&str]) -> io::Result<GhOutput>;

/// A token, held in memory. It has no `Serialize`, so no command or event can carry it.
pub struct Token(String);

impl Token {
    /// Asks `gh` for its token.
    pub fn from_gh(run: RunGh) -> Result<Self, Error> {
        let refused = || Error::new(ErrorKind::Unauthorized, SIGN_IN);
        let output = run(&["auth", "token", "--hostname", "github.com"]).map_err(|cause| {
            log::warn!("Tsugiki can't run gh: {cause}");
            refused()
        })?;
        let token = output.stdout.trim();
        if output.success && !token.is_empty() {
            logfile::hide(token);
            Ok(Self(token.to_owned()))
        } else {
            let what = if output.success {
                "printed no token"
            } else {
                "failed"
            };
            log::warn!("gh {what} when asked for its token");
            Err(refused())
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

#[cfg(test)]
mod tests;
