//! Whether a newer Tsugiki is out. A thread asks GitHub Releases at start-up and then at the interval the
//! app gives, with no token, and says when the latest release is newer than the running version. Tsugiki
//! doesn't update itself, so the status bar only says so and links to the release (ADR 0012).

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use specta::Type;
use tsugiki_token::{Reply, RestPath, Unreachable};

use crate::web::{TSUGIKI, WebAddress};

/// Asks GitHub for a path anyone may read, with no token: the token crate's `Https::get_public` in the app,
/// a stand-in in tests.
pub type AskPublic = Box<dyn FnMut(&RestPath) -> Result<Reply, Unreachable> + Send>;

/// Says that a newer version is out.
pub type AnnounceNewVersion = Box<dyn FnMut(&NewVersion) + Send>;

/// A version newer than the running one, and its release page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NewVersion {
    /// As SemVer writes it, such as "0.3.0", with no `v`.
    pub version: String,
    pub page: WebAddress,
}

impl NewVersion {
    fn of(version: &Version) -> Self {
        Self {
            version: version.to_string(),
            page: WebAddress::of_release(version),
        }
    }
}

/// The thread that checks. Dropping this ends the thread at once, or after the request it is waiting on.
pub struct ReleaseCheck {
    _stop: Sender<()>,
}

impl ReleaseCheck {
    /// Checks with `ask` now and then each `every`, and tells `announce` each time the latest release is a
    /// newer version than both the running one and the last it told. SemVer's precedence decides, so a tag
    /// that adds only build metadata, such as `v0.2.1+win`, is no newer. A failed check tells nothing, and
    /// the log says why. The next check tries again.
    pub fn start(mut ask: AskPublic, every: Duration, mut announce: AnnounceNewVersion) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let mut told = running();
        let _ = thread::spawn(move || {
            loop {
                match latest(&mut ask) {
                    Ok(version) if version.cmp_precedence(&told).is_gt() => {
                        announce(&NewVersion::of(&version));
                        told = version;
                    }
                    Ok(_) => {}
                    Err(why) => log::warn!("Tsugiki couldn't check for a new version: {why}"),
                }
                if stopped.recv_timeout(every) != Err(RecvTimeoutError::Timeout) {
                    break;
                }
            }
        });
        Self { _stop: stop }
    }
}

/// The version that runs. Cargo checks that `Cargo.toml`'s version is SemVer, so the fallback never applies.
fn running() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0))
}

/// The field of GitHub's answer about the latest release that the check reads.
#[derive(Deserialize)]
struct Answer {
    tag_name: String,
}

/// The version the release tag `tag` names. Tsugiki's tags are `v` and the version, such as `v0.3.0`, as
/// `release.yml` requires, and `WebAddress::of_release` builds the release page's address the same way.
fn tagged_version(tag: &str) -> Result<Version, String> {
    let Some(number) = tag.strip_prefix('v') else {
        return Err(format!("the tag {tag} doesn't start with v"));
    };
    Version::parse(number).map_err(|cause| format!("the tag {tag} isn't a version: {cause}"))
}

/// Asks `ask` for the version of the latest release, or says why it can't tell. GitHub leaves drafts and
/// prereleases out of this answer.
fn latest(ask: &mut AskPublic) -> Result<Version, String> {
    let latest = format!("/repos/{TSUGIKI}/releases/latest");
    let path = RestPath::parse(&latest).ok_or(format!("{latest} isn't a path"))?;
    let reply = ask(&path).map_err(|failure| match failure {
        Unreachable::Offline(cause) | Unreachable::CertificateDates(cause) => cause,
    })?;
    if reply.status != 200 {
        return Err(format!("GitHub answered {}", reply.status));
    }
    let answer: Answer = serde_json::from_str(&reply.body)
        .map_err(|cause| format!("GitHub's answer doesn't read as a release: {cause}"))?;
    tagged_version(&answer.tag_name)
}

#[cfg(test)]
mod tests;
