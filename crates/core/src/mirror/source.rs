//! A mirror's source of objects (#172): all from GitHub, or borrowed from a clone the user has, with
//! `git clone --reference --dissociate`. Tsugiki only reads the user's clone, and never searches the disk for
//! one.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::Git;
use crate::repository::RepositoryName;

/// The word `machine.toml` keeps for a mirror that downloads everything. Any other answer is a folder.
const DOWNLOAD: &str = "download";

/// The user's answer of where a Repository's mirror gets its objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MirrorSource {
    /// All from GitHub.
    Download,
    /// Objects from the clone in `folder` where it has them, and the rest from GitHub.
    Borrow { folder: PathBuf },
}

impl MirrorSource {
    /// The answer `machine.toml` keeps as `text`.
    pub(crate) fn from_text(text: &str) -> Self {
        if text == DOWNLOAD {
            Self::Download
        } else {
            Self::Borrow {
                folder: PathBuf::from(text),
            }
        }
    }

    /// This answer as `machine.toml` keeps it.
    pub(crate) fn to_text(&self) -> String {
        match self {
            Self::Download => DOWNLOAD.to_owned(),
            Self::Borrow { folder } => folder.display().to_string(),
        }
    }
}

/// Each Repository's answer on this machine. A Repository left out hasn't been asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorSources(Vec<(RepositoryName, MirrorSource)>);

impl MirrorSources {
    pub(crate) fn new(answers: Vec<(RepositoryName, MirrorSource)>) -> Self {
        Self(answers)
    }

    /// Keeps `source` as the answer for `repository`, in place of any before.
    pub(crate) fn set(&mut self, repository: RepositoryName, source: MirrorSource) {
        self.0.retain(|(one, _)| !one.is_same(&repository));
        self.0.push((repository, source));
    }

    /// The user's answer for `repository`, if they gave one.
    pub fn get(&self, repository: &RepositoryName) -> Option<&MirrorSource> {
        self.0
            .iter()
            .find(|(one, _)| one.is_same(repository))
            .map(|(_, source)| source)
    }
}

/// The reason a folder can't lend its objects to a Repository's mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloneRefusal {
    NotAFolder,
    /// git can't read a clone there, for the reason it gives in `git_says`.
    NotAClone {
        git_says: String,
    },
    /// None of its remotes is the Repository on GitHub.
    NoRemote,
    /// A shallow or grafted clone, which `git clone --reference` refuses.
    Shallow,
}

/// The git folder that holds the objects of the clone in `folder`, if it is a clone of `repository` at one of
/// `prefixes`: a worktree's is its common folder. It runs only git commands that read.
pub(crate) fn lender(
    git: &dyn Git,
    prefixes: &[String],
    folder: &Path,
    repository: &RepositoryName,
) -> Result<PathBuf, CloneRefusal> {
    if !folder.is_dir() {
        return Err(CloneRefusal::NotAFolder);
    }
    let in_folder = [OsStr::new("-C"), folder.as_os_str()];
    let common = git
        .read(
            &[
                &in_folder[..],
                &["rev-parse", "--git-common-dir"].map(OsStr::new),
            ]
            .concat(),
        )
        .map_err(|cause| CloneRefusal::NotAClone {
            git_says: cause.to_string(),
        })?;
    // git gives the folder relative to `folder` unless it is elsewhere, and joining an absolute path keeps
    // it.
    let common = folder.join(common.trim());
    // git answers nothing, and fails, when no remote has a URL.
    let urls = git
        .read(
            &[
                &in_folder[..],
                &["config", "--get-regexp", r"^remote\..*\.url$"].map(OsStr::new),
            ]
            .concat(),
        )
        .unwrap_or_default();
    let mut remotes = urls.lines().filter_map(|line| line.split_once(' '));
    if !remotes.any(|(_, url)| names(prefixes, url.trim(), repository)) {
        Err(CloneRefusal::NoRemote)
    } else if ["shallow", "info/grafts"]
        .iter()
        .any(|file| common.join(file).exists())
    {
        Err(CloneRefusal::Shallow)
    } else {
        Ok(common)
    }
}

/// Whether `url` is `repository` at one of `prefixes`, with or without `.git` and a user, in any case.
pub(super) fn names(prefixes: &[String], url: &str, repository: &RepositoryName) -> bool {
    let url = without_user(url);
    let url = url.as_str();
    prefixes.iter().any(|prefix| {
        let rest = url
            .get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .and_then(|_| url.get(prefix.len()..));
        let rest = rest.map(|rest| rest.trim_end_matches('/'));
        let rest = rest.map(|rest| rest.strip_suffix(".git").unwrap_or(rest));
        rest.and_then(|rest| RepositoryName::parse(rest).ok())
            .is_some_and(|named| named.is_same(repository))
    })
}

/// `url` without the user in `https://user@github.com/…` or `ssh://git@github.com/…`. The `git@` of
/// `git@github.com:…` stays, as it has no `://`.
fn without_user(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    format!("{scheme}://{host}{path}")
}

/// The prefixes a remote of a Repository on GitHub starts with, over HTTPS or SSH, once `without_user`
/// has dropped its user.
pub(crate) const GITHUB_PREFIXES: [&str; 7] = [
    "https://github.com/",
    "https://www.github.com/",
    "http://github.com/",
    "git@github.com:",
    "ssh://github.com/",
    "ssh://ssh.github.com:443/",
    "git://github.com/",
];
