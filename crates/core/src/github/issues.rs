//! The Issues that PRs name, fetched by number, since GitHub has no search for a list of numbers. A number
//! can turn out to be a PR or to name nothing, and then Tsugiki keeps nothing for it (ADR 0011). An Issue's
//! cross-references list the PRs that may belong in its tree.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use super::pull_requests::{NodeId, PageRequest, Person, RateLimit};
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
pub(super) struct IssueWire {
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

/// A PR that mentions an Issue. Only the link rules can say whether it belongs in the Issue's tree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedPr {
    pub id: NodeId,
    pub number: u32,
}

/// One page of an Issue's cross-references, the newest PR first.
#[derive(Debug, PartialEq, Eq)]
pub struct CrossReferences {
    /// Only PRs of this Repository: a PR elsewhere can't fix this Issue (ADR 0011).
    pub listed: Vec<ListedPr>,
    /// The next page's cursor, which holds older events, if there is one.
    pub next: Option<String>,
    pub rate_limit: RateLimit,
}

/// GitHub lists events oldest first, so `last` and `before` read them from the newest (ADR 0011).
const FETCH_CROSS_REFERENCES: &str = "query FetchCrossReferences($owner: String!, $name: String!, $number: Int!, $last: Int!, $before: String) { \
     repository(owner: $owner, name: $name) { issue(number: $number) { \
     timelineItems(last: $last, before: $before, itemTypes: [CROSS_REFERENCED_EVENT]) { \
     pageInfo { hasPreviousPage startCursor } nodes { ... on CrossReferencedEvent { source { __typename \
     ... on PullRequest { id number repository { nameWithOwner } } } } } } } } \
     rateLimit { remaining resetAt } }";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedRepository {
    name_with_owner: RepositoryName,
}

#[derive(Deserialize)]
struct SourcePr {
    #[serde(flatten)]
    listed: ListedPr,
    repository: NamedRepository,
}

/// The Issue or PR that mentioned the Issue.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Source {
    PullRequest(SourcePr),
    Issue,
}

#[derive(Deserialize)]
struct CrossReferenceEvent {
    source: Source,
}

impl GitHub {
    /// One page of the cross-references of Issue `number`, or half that page after a 502 or no answer. An
    /// Issue GitHub no longer has here, as after a transfer, has none.
    pub fn fetch_cross_references(
        &self,
        repository: &RepositoryName,
        number: u32,
        page: PageRequest<'_>,
    ) -> Result<CrossReferences, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PageInfo {
            has_previous_page: bool,
            start_cursor: Option<String>,
        }
        #[derive(Deserialize)]
        struct Timeline {
            #[serde(rename = "pageInfo")]
            page_info: PageInfo,
            nodes: Vec<Option<CrossReferenceEvent>>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Issue {
            timeline_items: Timeline,
        }
        #[derive(Deserialize)]
        struct Holder {
            issue: Option<Issue>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Holder,
            rate_limit: RateLimit,
        }
        let (data, _): (Data, usize) = self.query_at_size(page.size, |last| Request {
            operation: "FetchCrossReferences",
            query: FETCH_CROSS_REFERENCES.into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
                "last": last,
                "before": page.after,
            }),
        })?;
        let Some(issue) = data.repository.issue else {
            return Ok(CrossReferences {
                listed: Vec::new(),
                next: None,
                rate_limit: data.rate_limit,
            });
        };
        let timeline = issue.timeline_items;
        let info = timeline.page_info;
        let listed = timeline
            .nodes
            .into_iter()
            .rev()
            .flatten()
            .filter_map(|event| match event.source {
                Source::PullRequest(pr) if pr.repository.name_with_owner.is_same(repository) => {
                    Some(pr.listed)
                }
                Source::PullRequest(_) | Source::Issue => None,
            })
            .collect();
        Ok(CrossReferences {
            listed,
            next: info.start_cursor.filter(|_| info.has_previous_page),
            rate_limit: data.rate_limit,
        })
    }

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
            "author": { "login": "some-author" },
        })
    }

    fn event(source: &Value) -> Value {
        json!({ "source": source })
    }

    fn pr_source(number: u32, repository: &str) -> Value {
        json!({
            "__typename": "PullRequest",
            "id": format!("id{number}"),
            "number": number,
            "repository": { "nameWithOwner": repository },
        })
    }

    fn timeline(nodes: &[Value], previous: Option<&str>) -> Result<Reply, Unreachable> {
        let issue = json!({ "timelineItems": {
            "pageInfo": { "hasPreviousPage": previous.is_some(), "startCursor": previous.or(Some("start")) },
            "nodes": nodes,
        } });
        answer(&json!({ "issue": issue }))
    }

    fn page(after: Option<&str>) -> PageRequest<'_> {
        PageRequest { size: 50, after }
    }

    fn listed_numbers(found: &CrossReferences) -> Vec<u32> {
        found.listed.iter().map(|pr| pr.number).collect()
    }

    #[test]
    fn cross_references_list_this_repositorys_prs_newest_first() {
        let reply = timeline(
            &[
                event(&pr_source(1, "python/cpython")),
                event(&json!({ "__typename": "Issue" })),
                Value::Null,
                event(&pr_source(2, "python/peps")),
                event(&pr_source(3, "Python/CPython")),
            ],
            None,
        );
        let (script, asked) = Scripted::new(vec![reply]);

        let found = github(script)
            .fetch_cross_references(&cpython(), 7, page(Some("older")))
            .unwrap();

        assert_eq!(listed_numbers(&found), [3, 1]);
        assert_eq!(found.next, None);
        assert_eq!(found.rate_limit.remaining, 4000);
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "FetchCrossReferences");
        assert_eq!(
            variables,
            json!({
                "owner": "python",
                "name": "cpython",
                "number": 7,
                "last": 50,
                "before": "older",
            })
        );
    }

    #[test]
    fn cross_references_say_where_the_older_ones_start() {
        let reply = timeline(&[event(&pr_source(1, "python/cpython"))], Some("older"));
        let (script, _asked) = Scripted::new(vec![reply]);

        let found = github(script)
            .fetch_cross_references(&cpython(), 7, page(None))
            .unwrap();

        assert_eq!(found.next.as_deref(), Some("older"));
    }

    #[test]
    fn an_issue_github_no_longer_has_has_no_cross_references() {
        let body = format!(
            r#"{{"data":{{"repository":{{"issue":null}},{RATE}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","issue"]}}]}}"#
        );
        let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

        let found = github(script)
            .fetch_cross_references(&cpython(), 7, page(None))
            .unwrap();

        assert_eq!(listed_numbers(&found), [0_u32; 0]);
        assert_eq!(found.next, None);
    }

    #[test]
    fn a_page_of_cross_references_github_cant_answer_is_asked_for_at_half_size() {
        let bad_gateway = Ok(Reply {
            status: 502,
            reset: None,
            body: "Bad Gateway".to_owned(),
        });
        let (script, asked) = Scripted::new(vec![bad_gateway, timeline(&[], None)]);

        drop(
            github(script)
                .fetch_cross_references(&cpython(), 7, page(None))
                .unwrap(),
        );

        let lasts: Vec<Value> = asked
            .try_iter()
            .map(|(_, sent)| sent["last"].clone())
            .collect();
        assert_eq!(lasts, [json!(50), json!(25)]);
    }

    #[test]
    fn a_page_of_cross_references_that_fails_twice_is_the_failure() {
        let (script, _asked) = Scripted::new(vec![Err(Unreachable), Err(Unreachable)]);

        let refused = github(script)
            .fetch_cross_references(&cpython(), 7, page(None))
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::Offline);
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
                author: Some(Login::new("some-author")),
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
