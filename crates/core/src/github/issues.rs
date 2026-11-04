//! The Issues that PRs name, fetched by number, since GitHub has no search for a list of numbers. A number
//! can turn out to be a PR or to name nothing, and then Tsugiki keeps nothing for it (ADR 0011).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use super::pull_requests::{Person, RateLimit};
use super::{GitHub, Login, Request};
use crate::error::Error;
use crate::repository::RepositoryName;

/// An Issue a PR names, as Tsugiki keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFacts {
    pub number: u32,
    pub title: String,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    pub updated_at: OffsetDateTime,
}

/// One answer to a fetch by number.
#[derive(Debug)]
pub struct FetchedIssues {
    /// One for each number that names an Issue.
    pub issues: Vec<IssueFacts>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueWire {
    number: u32,
    title: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
    author: Option<Person>,
}

impl From<IssueWire> for IssueFacts {
    fn from(wire: IssueWire) -> Self {
        Self {
            number: wire.number,
            title: wire.title,
            author: wire.author.map(|person| person.login),
            updated_at: wire.updated_at,
        }
    }
}

/// GitHub's answer for one number.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Numbered {
    Issue(IssueWire),
    PullRequest,
}

/// The query for `numbers`, with one field for each, because GraphQL has no way to look up a list of numbers
/// and a `$` variable can't add a field. Only digits reach this text.
fn fetch_issues_query(numbers: &[u32]) -> String {
    let fields: Vec<String> = numbers
        .iter()
        .map(|number| {
            format!(
                "i{number}: issueOrPullRequest(number: {number}) {{ __typename ... on Issue {{ number \
                 title updatedAt author {{ login }} }} }}"
            )
        })
        .collect();
    format!(
        "query FetchIssues($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) \
         {{ {} }} rateLimit {{ remaining resetAt }} }}",
        fields.join(" ")
    )
}

impl GitHub {
    /// The first `size` of `numbers`, or half that after a 502 or no answer. `numbers` holds at least one.
    pub fn fetch_issues(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<FetchedIssues, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: BTreeMap<String, Option<Numbered>>,
            rate_limit: RateLimit,
        }
        let (data, asked_for): (Data, usize) =
            self.query_at_size(size.min(numbers.len()), |size| {
                let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
                Request {
                    operation: "FetchIssues",
                    query: fetch_issues_query(&asked).into(),
                    variables: json!({ "owner": repository.owner(), "name": repository.name() }),
                }
            })?;
        let issues = data
            .repository
            .into_values()
            .flatten()
            .filter_map(|numbered| match numbered {
                Numbered::Issue(wire) => Some(IssueFacts::from(wire)),
                Numbered::PullRequest => None,
            })
            .collect();
        Ok(FetchedIssues {
            issues,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::github::testing::{Scripted, reply_ok, token};
    use crate::github::{Reply, Unreachable};
    use serde_json::Value;
    use std::path::Path;
    use time::format_description::well_known::Rfc3339;

    const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn github(script: Scripted) -> GitHub {
        GitHub::new(Box::new(script), token())
    }

    fn answer(repository: &Value) -> Result<Reply, Unreachable> {
        reply_ok(&format!(
            r#"{{"data":{{"repository":{repository},{RATE}}}}}"#
        ))
    }

    fn issue_json(number: u32) -> Value {
        json!({
            "__typename": "Issue",
            "number": number,
            "title": format!("Issue {number}"),
            "updatedAt": "2026-11-04T10:00:00Z",
            "author": { "login": "vstinner" },
        })
    }

    #[test]
    fn a_fetched_issue_is_read_into_its_facts() {
        let reply = answer(&json!({ "i7": issue_json(7) }));
        let (script, asked) = Scripted::new(vec![reply]);

        let fetched = github(script).fetch_issues(&cpython(), &[7], 25).unwrap();

        assert_eq!(
            fetched.issues,
            [IssueFacts {
                number: 7,
                title: "Issue 7".to_owned(),
                author: Some(Login::new("vstinner")),
                updated_at: OffsetDateTime::parse("2026-11-04T10:00:00Z", &Rfc3339).unwrap(),
            }]
        );
        assert_eq!(fetched.asked_for, 1);
        assert_eq!(fetched.rate_limit.remaining, 4000);
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "FetchIssues");
        assert_eq!(variables, json!({ "owner": "python", "name": "cpython" }));
    }

    #[test]
    fn an_issue_whose_author_has_gone_has_no_author() {
        let mut issue = issue_json(7);
        issue["author"] = Value::Null;
        let (script, _asked) = Scripted::new(vec![answer(&json!({ "i7": issue }))]);

        let fetched = github(script).fetch_issues(&cpython(), &[7], 25).unwrap();

        assert_eq!(fetched.issues[0].author, None);
    }

    #[test]
    fn a_number_that_is_a_pr_or_names_nothing_leaves_no_issue() {
        let body = format!(
            r#"{{"data":{{"repository":{},{RATE}}},"errors":[{{"type":"NOT_FOUND","message":"gone",
                "path":["repository","i3"]}}]}}"#,
            json!({ "i1": issue_json(1), "i2": { "__typename": "PullRequest" }, "i3": null })
        );
        let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

        let fetched = github(script)
            .fetch_issues(&cpython(), &[1, 2, 3], 25)
            .unwrap();

        let numbers: Vec<u32> = fetched.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, [1]);
        assert_eq!(fetched.asked_for, 3);
    }

    #[test]
    fn a_repository_that_has_gone_is_still_an_error() {
        let body = r#"{"data":{"repository":null},"errors":[{"type":"NOT_FOUND","message":"gone",
                       "path":["repository"]}]}"#;
        let (script, _asked) = Scripted::new(vec![reply_ok(body)]);

        let refused = github(script)
            .fetch_issues(&cpython(), &[1], 25)
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::NotFound);
    }

    #[test]
    fn a_batch_asks_for_its_size_and_a_retry_for_half() {
        let replies = vec![Err(Unreachable), answer(&json!({ "i1": issue_json(1) }))];
        let (script, _asked) = Scripted::new(replies);

        let fetched = github(script)
            .fetch_issues(&cpython(), &[1, 2, 3, 4], 2)
            .unwrap();

        assert_eq!(fetched.asked_for, 1);
    }

    #[test]
    fn the_query_names_each_number_and_opens_with_its_operation() {
        let query = fetch_issues_query(&[12, 34]);
        assert!(query.starts_with("query FetchIssues("), "{query}");
        assert!(
            query.contains("i12: issueOrPullRequest(number: 12)"),
            "{query}"
        );
        assert!(
            query.contains("i34: issueOrPullRequest(number: 34)"),
            "{query}"
        );
    }

    #[test]
    fn the_recording_holds_the_query_the_client_sends() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github/FetchIssues.json");
        let recording: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let recorded = recording["query"].as_str().unwrap();
        let numbers: Vec<u32> = recorded
            .split("number: ")
            .skip(1)
            .map(|rest| rest.split(')').next().unwrap().parse().unwrap())
            .collect();

        assert!(!numbers.is_empty());
        assert_eq!(recorded, fetch_issues_query(&numbers));
    }
}
