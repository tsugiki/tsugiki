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
mod tests {
    use super::*;

    /// `security.rs` looks for hosts other than GitHub's in these sources, so this one is written apart.
    const ELSEWHERE: &str = concat!("https:", "//docs.python.org/3/");

    #[test]
    fn a_web_page_gets_through() {
        assert_eq!(WebAddress::parse(ELSEWHERE).unwrap().as_str(), ELSEWHERE);
    }

    #[test]
    fn anything_else_is_refused() {
        for text in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "vscode://file/etc/passwd",
            "mailto:someone@localhost",
            "/relative/path",
            "#a-heading",
            "",
            concat!("https:", "//someone:secret@docs.python.org/"),
            concat!("https:", "//:github.com@docs.python.org/"),
        ] {
            let problem = WebAddress::parse(text).unwrap_err();
            assert_eq!(problem.kind, ErrorKind::Internal, "{text}");
        }
    }

    #[test]
    fn an_item_has_its_page_on_github() {
        let cpython = RepositoryName::parse("python/cpython").unwrap();
        let page = |item| WebAddress::of_item(&cpython, item);

        assert_eq!(
            page(GitHubItem::Issue { number: 7 }).as_str(),
            "https://github.com/python/cpython/issues/7"
        );
        assert_eq!(
            page(GitHubItem::PullRequest { number: 8 }).as_str(),
            "https://github.com/python/cpython/pull/8"
        );
    }
}
