//! Fetching one Issue or PR by its number, whichever it turns out to be, for an item the database lacks
//! (#981), and a batch of them for the placeholders an Inbox search left (#466). GitHub shares one number
//! space between Issues and PRs, so one field asks for either.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::issues::{ISSUE_FIELDS, IssueWire};
use super::pull_requests::{PULL_REQUEST_FIELDS, PullRequestWire, RateLimit};
use super::{GitHub, RATE_LIMIT_FIELDS, Request};
use crate::error::Error;
use crate::facts::activity::ItemActivity;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::PullRequestFacts;
use crate::repository::RepositoryName;

/// The field under `repository` that asks for the item, which GitHub can refuse alone.
const FIELD: &str = "issueOrPullRequest";

/// Names the fetch of one item, and so its recording.
const OPERATION: &str = "FetchItem";

/// Names the fetch of a batch of items, whose query names each number, so it has no recording.
const BATCH_OPERATION: &str = "FetchItems";

/// The alias a query gives item `number`, which its answer comes back under.
pub(crate) fn alias(number: u32) -> String {
    format!("i{number}")
}

/// What a fetch of one item reads, whichever kind GitHub says it is, and the Repository that holds it now.
fn item_selection() -> String {
    format!(
        "__typename ... on Issue {{ repository {{ nameWithOwner }} {ISSUE_FIELDS} }} \
         ... on PullRequest {{ {PULL_REQUEST_FIELDS} }}"
    )
}

fn fetch_item_query() -> String {
    let selection = item_selection();
    format!(
        "query {OPERATION}($owner: String!, $name: String!, $number: Int!) {{ \
         repository(owner: $owner, name: $name) {{ nameWithOwner {FIELD}(number: $number) {{ {selection} }} }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

/// The text of a fetch of `numbers`, one field for each, as GraphQL can't look up a list by variable. Only
/// digits reach this text.
fn fetch_items_query(numbers: &[u32]) -> String {
    let selection = item_selection();
    let fields: Vec<String> = numbers
        .iter()
        .map(|number| {
            let alias = alias(*number);
            format!("{alias}: {FIELD}(number: {number}) {{ {selection} }}")
        })
        .collect();
    format!(
        "query {BATCH_OPERATION}($owner: String!, $name: String!) {{ \
         repository(owner: $owner, name: $name) {{ nameWithOwner {} }} {RATE_LIMIT_FIELDS} }}",
        fields.join(" ")
    )
}

/// The Repository an answer names, which GitHub finds under an old name too.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Holdings {
    name_with_owner: RepositoryName,
}

/// An Issue, with the Repository that holds it now.
#[derive(Deserialize)]
struct HeldIssue {
    repository: Holdings,
    #[serde(flatten)]
    issue: IssueWire,
}

/// One item, whichever kind GitHub says it is.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Wire {
    Issue(HeldIssue),
    PullRequest(Box<PullRequestWire>),
}

/// What GitHub said number N of a Repository is.
#[derive(Debug, PartialEq, Eq)]
pub enum Numbered {
    Issue(IssueFacts, ItemActivity),
    /// Boxed, as a PR's facts are many times the size of the other answers.
    PullRequest(Box<PullRequestFacts>, ItemActivity),
    /// GitHub has nothing under that number here. An Issue transferred to another Repository answers from
    /// there, under the number it has there, which can be the same, and counts as nothing here too.
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

/// The answer to a fetch of a batch of items by their numbers, in the order they were asked.
#[derive(Debug, PartialEq, Eq)]
pub struct FetchedItems {
    pub items: Vec<Numbered>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
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
        #[serde(rename_all = "camelCase")]
        struct Holder {
            name_with_owner: RepositoryName,
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
        let asked = data.repository.name_with_owner;
        let numbered = match data.repository.item {
            _ if !refused.is_empty() => {
                for refusal in refused {
                    log::warn!("GitHub wouldn't give #{number}: {}", refusal.message);
                }
                Numbered::Hidden
            }
            Some(Wire::Issue(held)) if held.repository.name_with_owner.is_same(&asked) => {
                let (facts, activity) = held.issue.into();
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

    /// The first `size` of `numbers`, or half that after a 502 or no answer, as `fetch_item` does one: the
    /// item facts of the placeholders an Inbox search left, without their Conversations (#466). A number
    /// GitHub refuses alone answers `Numbered::Hidden`, and the rest of the batch stands.
    pub fn fetch_items(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<FetchedItems, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Holder {
            name_with_owner: RepositoryName,
            #[serde(flatten)]
            items: BTreeMap<String, Option<Wire>>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Holder,
            rate_limit: RateLimit,
        }
        let aliases: Vec<String> = numbers.iter().map(|number| alias(*number)).collect();
        let reply = self.read_at_size(
            size.min(numbers.len()),
            |first| {
                let asked: Vec<u32> = numbers.iter().take(first).copied().collect();
                Request {
                    operation: BATCH_OPERATION,
                    query: fetch_items_query(&asked).into(),
                    variables: json!({ "owner": repository.owner(), "name": repository.name() }),
                }
            },
            |reply| {
                let refusable = |field: &str| aliases.iter().any(|asked| asked == field);
                self.answer_with_refused(reply, refusable)
            },
        );
        let ((mut data, refused), asked_for): ((Data, Vec<_>), usize) = reply?;
        let mut hidden: Vec<String> = refused
            .into_iter()
            .map(|refusal| {
                log::warn!(
                    "GitHub wouldn't give one item of {}: {}",
                    repository,
                    refusal.message
                );
                refusal.field
            })
            .collect();
        let asked = data.repository.name_with_owner;
        let mut items = Vec::new();
        for number in numbers.iter().take(asked_for) {
            let name = alias(*number);
            let refused = hidden.iter().any(|field| field == &name);
            hidden.retain(|field| field != &name);
            let answer = (data.repository.items).remove(&name).flatten();
            let numbered = match answer {
                _ if refused => Numbered::Hidden,
                Some(Wire::Issue(held)) if held.repository.name_with_owner.is_same(&asked) => {
                    let (facts, activity) = held.issue.into();
                    Numbered::Issue(facts, activity)
                }
                Some(Wire::PullRequest(pull_request)) => {
                    let (facts, activity) = (*pull_request).into();
                    Numbered::PullRequest(Box::new(facts), activity)
                }
                Some(Wire::Issue(_)) | None => Numbered::Missing,
            };
            items.push(numbered);
        }
        Ok(FetchedItems {
            items,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
