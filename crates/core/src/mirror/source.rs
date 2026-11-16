//! A clone the user has that lends its objects to a mirror (#172), with `git clone --reference
//! --dissociate`. Tsugiki only reads the user's clone, and never searches the disk for one. The user's answer
//! of where each mirror gets its objects is a `MirrorSource`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;

use super::Git;
use crate::repository::RepositoryName;

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
