//! What the core's tests share, and share with the tests of the crates that use the core. Those reach it
//! through the `testing` feature, which only their `[dev-dependencies]` turn on, so no release build has
//! it. What needs `tsugiki-test-git`, which only tests may depend on, is for the core's own tests.

#![expect(clippy::unwrap_used, reason = "a failed setup step ends the test")]

use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::facts::CommitSha;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::folders::Folders;
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;

#[cfg(test)]
mod fixture;
#[cfg(test)]
pub(crate) mod import;

#[cfg(test)]
pub(crate) use fixture::Fixture;
/// A test's folder or file under the repo's ignored `.temp` folder, removed when the test passes.
#[cfg(test)]
pub(crate) use tsugiki_test_git::{TempPath, temp_path};

/// Adds `text` to the config of the repository whose git folder is `git_folder`, as `git config` writes it,
/// without a git to write it, since starting one is slow on Windows (ADR 0003).
#[cfg(test)]
pub(crate) fn add_config(git_folder: &Path, text: &str) {
    use std::io::Write as _;
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(git_folder.join("config"))
        .unwrap();
    config.write_all(text.as_bytes()).unwrap();
}

/// The Repository a test works in.
pub fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// [`cpython`]'s settings, as a Repository just added has them.
pub fn cpython_settings() -> RepositorySettings {
    RepositorySettings::new(cpython())
}

/// The user signed in.
pub fn me() -> Login {
    Login::new("octocat")
}

/// The commit `text` names in 40 hex digits, with any white space around them dropped.
pub fn sha(text: &str) -> CommitSha {
    CommitSha::parse(text.trim()).unwrap()
}

/// The commit whose SHA is 40 of `digit`.
pub fn sha_of(digit: char) -> CommitSha {
    sha(&digit.to_string().repeat(40))
}

/// The moment `text` names in RFC 3339, as GitHub writes one.
pub fn moment(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// `hours` before now, to the second, as GitHub and the store keep a moment.
pub fn hours_ago(hours: i64) -> OffsetDateTime {
    let at = OffsetDateTime::now_utc() - Duration::hours(hours);
    at.replace_nanosecond(0).unwrap()
}

/// An open PR `number` to main, with no author or anything else: a base whose fields a test overrides.
pub fn pr_facts(number: u32) -> PullRequestFacts {
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

/// An open Issue `number`, with no author or anything else: a base whose fields a test overrides.
pub fn issue_facts(number: u32) -> IssueFacts {
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

/// Folders that are all `folder`: the data, the settings and the logs.
pub fn one_folder(folder: &Path) -> Folders {
    Folders {
        data: folder.to_path_buf(),
        settings: folder.to_path_buf(),
        logs: folder.to_path_buf(),
    }
}

/// Where `npm run record` keeps real GitHub's answers, which the tests replay (ADR 0009).
pub fn recordings() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github")
}
