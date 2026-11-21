//! Fetching one Issue or PR by its number, whichever it turns out to be, for an item the database lacks
//! (#981). GitHub shares one number space between Issues and PRs, so one field asks for either.

use serde::Deserialize;
use serde_json::json;

use super::issues::{AnsweredIssue as _, ISSUE_FIELDS, IssueWire};
use super::pull_requests::{PULL_REQUEST_FIELDS, PullRequestWire, RateLimit};
use super::{GitHub, RATE_LIMIT_FIELDS, Request};
use crate::error::Error;
use crate::facts::activity::ItemActivity;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::PullRequestFacts;
use crate::repository::RepositoryName;

/// The field under `repository` that asks for the item, which GitHub can refuse alone.
const FIELD: &str = "issueOrPullRequest";

/// Names the fetch, and so its recording.
const OPERATION: &str = "FetchItem";

fn fetch_item_query() -> String {
    format!(
        "query {OPERATION}($owner: String!, $name: String!, $number: Int!) {{ \
         repository(owner: $owner, name: $name) {{ {FIELD}(number: $number) {{ __typename \
         ... on Issue {{ {ISSUE_FIELDS} }} ... on PullRequest {{ {PULL_REQUEST_FIELDS} }} }} }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

/// What GitHub said number N of a Repository is.
#[derive(Debug, PartialEq, Eq)]
pub enum Numbered {
    Issue(IssueFacts, ItemActivity),
    /// Boxed, as a PR's facts are many times the size of the other answers.
    PullRequest(Box<PullRequestFacts>, ItemActivity),
    /// GitHub has nothing under that number here. An Issue transferred to another Repository answers under
    /// its number there, and counts as nothing here too.
    Missing,
    /// GitHub refused that one field, as it does when the token may see the Repository but not the item.
    Hidden,
}

/// The answer to a fetch of one item by its number.
#[derive(Debug, PartialEq, Eq)]
pub struct FetchedItem {
    pub numbered: Numbered,
    pub rate_limit: RateLimit,
}

impl GitHub {
    /// Issue or PR `number` of `repository`, whichever it is. A Repository the token can't see fails with
    /// `ErrorKind::NotFound`.
    pub fn fetch_item(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<FetchedItem, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "__typename")]
        enum Wire {
            Issue(IssueWire),
            PullRequest(Box<PullRequestWire>),
        }
        #[derive(Deserialize)]
        struct Holder {
            #[serde(rename = "issueOrPullRequest")]
            item: Option<Wire>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Holder,
            rate_limit: RateLimit,
        }
        let reply = self.post(&Request {
            operation: OPERATION,
            query: fetch_item_query().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
            }),
        })?;
        let (data, refused): (Data, _) =
            self.answer_with_refused(&reply, |field| field == FIELD)?;
        let numbered = match data.repository.item {
            _ if !refused.is_empty() => {
                for refusal in refused {
                    log::warn!("GitHub wouldn't give #{number}: {}", refusal.message);
                }
                Numbered::Hidden
            }
            Some(Wire::Issue(issue)) if issue.number() == number => {
                let (facts, activity) = issue.into();
                Numbered::Issue(facts, activity)
            }
            Some(Wire::PullRequest(pull_request)) => {
                let (facts, activity) = (*pull_request).into();
                Numbered::PullRequest(Box::new(facts), activity)
            }
            Some(Wire::Issue(_)) | None => Numbered::Missing,
        };
        Ok(FetchedItem {
            numbered,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
