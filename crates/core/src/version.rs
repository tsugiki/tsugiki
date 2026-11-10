//! Which code runs, for the About line, the log and a HUD recording, so a bug report says what was built
//! (#238).

use std::fmt;

/// The version `Cargo.toml` gives, and the commit the build came from if the build could ask git. It reads
/// "0.1.1 (3a6dc89)", or "0.1.1" alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The SHA's first 7 characters, as GitHub shows them.
    commit: Option<String>,
}

impl Version {
    /// How many characters of the SHA GitHub shows, and the fewest git's `rev-parse` gives.
    const SHORT: usize = 7;

    /// `sha` is what git's `rev-parse HEAD` printed: the core can't ask, or it would rebuild for each commit,
    /// so the app's build script asks and hands it over. A text that isn't a SHA counts as no commit.
    pub fn new(sha: Option<&str>) -> Self {
        let commit = sha
            .filter(|sha| sha.len() >= Self::SHORT && sha.chars().all(|c| c.is_ascii_hexdigit()))
            .and_then(|sha| sha.get(..Self::SHORT))
            .map(str::to_ascii_lowercase);
        Self { commit }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(env!("CARGO_PKG_VERSION"))?;
        match &self.commit {
            Some(commit) => write!(f, " ({commit})"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
