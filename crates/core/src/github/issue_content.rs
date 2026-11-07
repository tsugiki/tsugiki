//! The description and recent comments of one Issue or PR, as GitHub renders them. Only the Issue view and
//! the PR view's Conversation show them, so Tsugiki asks when one opens and while it's on screen, and no sync
//! pays for them (ADR 0015).

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::pull_requests::Person;
use super::{GitHub, Login, Request};
use crate::error::{Error, ErrorKind};
use crate::repository::RepositoryName;

/// Whose conversation a query reads. GitHub gives an Issue and a PR the same description and comments.
#[derive(Debug, Clone, Copy)]
enum Thread {
    Issue,
    PullRequest,
}

impl Thread {
    const fn operation(self) -> &'static str {
        match self {
            Self::Issue => "FetchIssueContent",
            Self::PullRequest => "FetchPullRequestContent",
        }
    }

    /// The field of `repository` that holds it.
    const fn field(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::PullRequest => "pullRequest",
        }
    }

    const fn noun(self) -> &'static str {
        match self {
            Self::Issue => "Issue",
            Self::PullRequest => "PR",
        }
    }

    fn query(self) -> String {
        format!(
            "query {}($owner: String!, $name: String!, $number: Int!) {{ \
             repository(owner: $owner, name: $name) {{ {}(number: $number) {{ bodyHTML \
             comments(last: 30) {{ totalCount nodes {{ author {{ login }} createdAt bodyHTML }} }} }} }} }}",
            self.operation(),
            self.field(),
        )
    }
}

/// A comment on an Issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueComment {
    /// Nothing, for a comment whose author's account has gone.
    pub author: Option<Login>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub created_at: OffsetDateTime,
    /// GitHub's rendering, written by a stranger. The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
}

/// An Issue's description and its recent comments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueContent {
    /// GitHub's rendering, written by a stranger. The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
    /// The most recent comments, oldest first.
    pub comments: Vec<IssueComment>,
    /// The count of older comments that only GitHub shows.
    pub earlier_comments: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentWire {
    author: Option<Person>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(rename = "bodyHTML")]
    body_html: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentsWire {
    total_count: u32,
    nodes: Vec<Option<CommentWire>>,
}

#[derive(Deserialize)]
struct IssueWire {
    #[serde(rename = "bodyHTML")]
    body_html: String,
    comments: CommentsWire,
}

impl From<IssueWire> for IssueContent {
    fn from(wire: IssueWire) -> Self {
        let comments: Vec<IssueComment> = wire
            .comments
            .nodes
            .into_iter()
            .flatten()
            .map(|comment| IssueComment {
                author: comment.author.map(|person| person.login),
                created_at: comment.created_at,
                body_html: comment.body_html,
            })
            .collect();
        let held = u32::try_from(comments.len()).unwrap_or(u32::MAX);
        Self {
            body_html: wire.body_html,
            comments,
            earlier_comments: wire.comments.total_count.saturating_sub(held),
        }
    }
}

impl GitHub {
    /// The description and recent comments of Issue `number`.
    pub fn issue_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<IssueContent, Error> {
        self.conversation(Thread::Issue, repository, number)
    }

    /// The description and recent comments of PR `number`: its Conversation, without its review threads.
    pub fn pull_request_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<IssueContent, Error> {
        self.conversation(Thread::PullRequest, repository, number)
    }

    fn conversation(
        &self,
        thread: Thread,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<IssueContent, Error> {
        #[derive(Deserialize)]
        struct Data {
            repository: RepositoryWire,
        }
        #[derive(Deserialize)]
        struct RepositoryWire {
            #[serde(alias = "pullRequest")]
            issue: Option<IssueWire>,
        }
        let data: Data = self.query(&Request {
            operation: thread.operation(),
            query: thread.query().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
            }),
        })?;
        let wire = data.repository.issue.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                &format!("GitHub has no {} #{number} in {repository}.", thread.noun()),
            )
        })?;
        Ok(wire.into())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use std::sync::mpsc::Receiver;
    use time::format_description::well_known::Rfc3339;

    use super::*;
    use crate::github::testing::{Scripted, reply_ok, token};

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn moment(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    /// A `GitHub` that gives one `issue`, and what it was asked.
    fn answering(issue: &Value) -> (GitHub, Receiver<(String, Value)>) {
        let body = json!({ "data": { "repository": { "issue": issue } } }).to_string();
        let (script, asked) = Scripted::new(vec![reply_ok(&body)]);
        (GitHub::new(Box::new(script), token()), asked)
    }

    #[test]
    fn an_issue_reads_as_its_rendering_with_the_comments_oldest_first() {
        let (github, asked) = answering(&json!({
            "bodyHTML": "<p>It crashes</p>",
            "comments": {
                "totalCount": 32,
                "nodes": [
                    {
                        "author": { "login": "some-reviewer" },
                        "createdAt": "2026-11-04T10:00:00Z",
                        "bodyHTML": "<p>Which build?</p>",
                    },
                    null,
                    { "author": null, "createdAt": "2026-11-04T11:00:00Z", "bodyHTML": "<p>Debug</p>" },
                ],
            },
        }));

        let content = github.issue_content(&cpython(), 7).unwrap();

        assert_eq!(
            content,
            IssueContent {
                body_html: "<p>It crashes</p>".to_owned(),
                comments: vec![
                    IssueComment {
                        author: Some(Login::new("some-reviewer")),
                        created_at: moment("2026-11-04T10:00:00Z"),
                        body_html: "<p>Which build?</p>".to_owned(),
                    },
                    IssueComment {
                        author: None,
                        created_at: moment("2026-11-04T11:00:00Z"),
                        body_html: "<p>Debug</p>".to_owned(),
                    },
                ],
                earlier_comments: 30,
            }
        );
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "FetchIssueContent");
        assert_eq!(variables["number"], 7);
        assert_eq!(variables["owner"], "python");
    }

    #[test]
    fn an_issue_github_doesnt_have_is_not_found() {
        let (github, _asked) = answering(&Value::Null);

        let problem = github.issue_content(&cpython(), 7).unwrap_err();

        assert_eq!(problem.kind, ErrorKind::NotFound);
        assert!(problem.message.contains("#7"), "{}", problem.message);
    }
    #[test]
    fn a_pr_github_doesnt_have_is_not_found() {
        let body = json!({ "data": { "repository": { "pullRequest": null } } }).to_string();
        let (script, asked) = Scripted::new(vec![reply_ok(&body)]);
        let github = GitHub::new(Box::new(script), token());

        let problem = github.pull_request_content(&cpython(), 7).unwrap_err();

        assert_eq!(problem.kind, ErrorKind::NotFound);
        assert!(problem.message.contains("PR #7"), "{}", problem.message);
        assert_eq!(asked.recv().unwrap().0, "FetchPullRequestContent");
    }
}
