//! Which code runs, for the About line, the log and a HUD recording, so a bug report says what was built
//! (#238).

use std::fmt;

use crate::github::CommitSha;

/// The version `Cargo.toml` gives, and the commit the build came from if the build could ask git. It reads
/// "0.1.1 (3a6dc89)", or "0.1.1" alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    commit: Option<CommitSha>,
}

impl Version {
    /// The core can't know the commit, or it would rebuild for each one: the app's build script asks git and
    /// hands it over.
    pub const fn new(commit: Option<CommitSha>) -> Self {
        Self { commit }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(env!("CARGO_PKG_VERSION"))?;
        match &self.commit {
            // A SHA has at least 7 characters, all ASCII, so `get` always finds them.
            Some(commit) => write!(f, " ({})", commit.as_str().get(..7).unwrap_or_default()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
