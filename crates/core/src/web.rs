//! Addresses Tsugiki may hand to the system browser. The webview shows HTML written by strangers, so a link
//! it asks to open counts as untrusted input (ADR 0005): only a web page gets through, never a file or an
//! installed app's scheme.

use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use crate::error::{Error, ErrorKind};
use crate::repository::RepositoryName;

/// An Issue or a PR, as the window names it to open on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitHubItem {
    Issue { number: u32 },
    PullRequest { number: u32 },
}

impl GitHubItem {
    #[must_use]
    pub fn number(self) -> u32 {
        match self {
            Self::Issue { number } | Self::PullRequest { number } => number,
        }
    }
}

/// A web page's address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(transparent)]
pub struct WebAddress(String);

impl WebAddress {
    /// Reads a link, and refuses anything but an `http` or `https` address with a host. An address that names
    /// a user or a password can pass for another site, so that's refused too.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let refused = || {
            Error::new(
                ErrorKind::Internal,
                "Tsugiki opens only web links in the browser.",
            )
        };
        let address = Url::parse(text).map_err(|cause| {
            log::warn!("A link Tsugiki can't read: {cause}");
            refused()
        })?;
        let is_web = matches!(address.scheme(), "http" | "https");
        let names_nobody = address.username().is_empty() && address.password().is_none();
        if is_web && address.has_host() && names_nobody {
            Ok(Self(address.into()))
        } else {
            Err(refused())
        }
    }

    /// The page of `item` on GitHub.
    #[must_use]
    pub fn of_item(repository: &RepositoryName, item: GitHubItem) -> Self {
        Self(match item {
            GitHubItem::Issue { number } => {
                format!("https://github.com/{repository}/issues/{number}")
            }
            GitHubItem::PullRequest { number } => {
                format!("https://github.com/{repository}/pull/{number}")
            }
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
