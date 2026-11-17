use std::fs;
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use tsugiki_test_git::git;

/// A test's folder or file under the repo's ignored `.temp` folder, removed when the test passes.
pub(crate) use tsugiki_test_git::{TempPath, temp_path};

use crate::facts::CommitSha;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::login::Login;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;

/// The Repository a test works in.
pub(crate) fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// [`cpython`]'s settings, as a Repository just added has them.
pub(crate) fn cpython_settings() -> RepositorySettings {
    RepositorySettings::new(cpython())
}

/// The user signed in.
pub(crate) fn me() -> Login {
    Login::new("octocat")
}

/// The commit `text` names in 40 hex digits, with any white space around them dropped.
pub(crate) fn sha(text: &str) -> CommitSha {
    CommitSha::parse(text.trim()).unwrap()
}

/// The commit whose SHA is 40 of `digit`.
pub(crate) fn sha_of(digit: char) -> CommitSha {
    sha(&digit.to_string().repeat(40))
}

/// The moment `text` names in RFC 3339, as GitHub writes one.
pub(crate) fn moment(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// `hours` before now, to the second, as GitHub and the store keep a moment.
pub(crate) fn hours_ago(hours: i64) -> OffsetDateTime {
    let at = OffsetDateTime::now_utc() - Duration::hours(hours);
    at.replace_nanosecond(0).unwrap()
}

/// An open PR `number` to main, with no author or anything else: a base whose fields a test overrides.
pub(crate) fn pr_facts(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        state: PrState::Open,
        is_draft: false,
        author: None,
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: None,
        off_default: None,
    }
}

/// A repository whose `main` is a PR's base and whose PR's head changes, adds and deletes files, and a
/// Lookup checkout at its base as the Mirror worker leaves one, built with the real git (ADR 0009).
pub(crate) struct Fixture {
    /// The repository the PR is in, which stands in for its Mirror.
    pub(crate) mirror: PathBuf,
    pub(crate) checkout: LookupCheckout,
    /// The folder around both, which a test drops after the searches and servers that work in it.
    _root: TempPath,
}

impl Fixture {
    /// [`Fixture::of_bytes`] for files of text.
    pub(crate) fn new(name: &str, base: &[(&str, &str)], head: &[(&str, Option<&str>)]) -> Self {
        let base: Vec<(&str, &[u8])> = (base.iter())
            .map(|(path, text)| (*path, text.as_bytes()))
            .collect();
        let head: Vec<(&str, Option<&[u8]>)> = (head.iter())
            .map(|(path, text)| (*path, text.map(str::as_bytes)))
            .collect();
        Self::of_bytes(name, &base, &head)
    }

    /// The base holds `base`, and the PR's head writes each of `head`'s files, or deletes it where its bytes
    /// are None. The checkout holds the base and the head's added files, as a move leaves it.
    pub(crate) fn of_bytes(
        name: &str,
        base: &[(&str, &[u8])],
        head: &[(&str, Option<&[u8]>)],
    ) -> Self {
        // The folder keeps the `..` in `temp_path`'s path, which the core reads past.
        let root = temp_path(name);
        let mirror = root.join("mirror");
        fs::create_dir_all(&mirror).unwrap();
        let _ = git(&mirror, &["init", "-q", "-b", "main"]);
        let _ = git(&mirror, &["config", "core.autocrlf", "false"]);
        for (path, bytes) in base {
            write(&mirror, path, bytes);
        }
        let base_commit = commit(&mirror, "base");
        let mut added = Vec::new();
        for (path, bytes) in head {
            match bytes {
                Some(bytes) => {
                    if !mirror.join(path).exists() {
                        added.push(*path);
                    }
                    write(&mirror, path, bytes);
                }
                None => {
                    let _ = git(&mirror, &["rm", "-q", path]);
                }
            }
        }
        let head_commit = commit(&mirror, "head");
        let folder = root.join("lookup");
        let folder_arg = folder.to_string_lossy().into_owned();
        let at = [
            "worktree",
            "add",
            "-q",
            "--detach",
            &folder_arg,
            base_commit.as_str(),
        ];
        let _ = git(&mirror, &at);
        if !added.is_empty() {
            let mut args = vec!["checkout", head_commit.as_str(), "--"];
            args.extend(added);
            let _ = git(&folder, &args);
        }
        Self {
            mirror,
            checkout: LookupCheckout {
                folder,
                at: LookupAt {
                    base: base_commit,
                    head: head_commit,
                },
            },
            _root: root,
        }
    }
}

fn write(folder: &Path, path: &str, bytes: &[u8]) {
    let file = folder.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, bytes).unwrap();
    let _ = git(folder, &["add", path]);
}

fn commit(folder: &Path, message: &str) -> CommitSha {
    let _ = git(folder, &["commit", "-q", "--allow-empty", "-m", message]);
    sha(&git(folder, &["rev-parse", "HEAD"]))
}

/// An open Issue `number`, with no author or anything else: a base whose fields a test overrides.
pub(crate) fn issue_facts(number: u32) -> IssueFacts {
    IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: None,
        assignees: vec![],
        labels: vec![],
        updated_at: OffsetDateTime::UNIX_EPOCH,
        state: Some(IssueState::Open),
    }
}
