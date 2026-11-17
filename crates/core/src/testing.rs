use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// A test's folder or file under the repo's ignored `.temp` folder, removed when the test passes.
pub(crate) use tsugiki_test_git::{TempPath, temp_path};

use crate::facts::CommitSha;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::login::Login;
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
