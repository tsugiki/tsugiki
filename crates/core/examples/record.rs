//! Record mode (ADR 0009): `npm run record` asks real GitHub the client's read-only queries, with gh's
//! token, and writes the scrubbed answers to `tests/fixtures/github`. Read them before you commit them.

#![expect(
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a developer's command line: a failure should stop it and say why in the terminal"
)]

use std::collections::BTreeSet;
use std::path::Path;

use tsugiki_core::github::GitHub;
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::github::pull_requests::PageRequest;
use tsugiki_core::github::record::Recorder;
use tsugiki_core::links::possible_numbers;
use tsugiki_core::repository::RepositoryName;
use tsugiki_core::sync::{FETCH_BATCH, SEARCH_PAGE, WORK_SEARCHES};
use tsugiki_core::token::Token;

/// The Repository the first sync's recordings come from, which every signed-in user can read.
const WORK_REPOSITORY: &str = "python/cpython";

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let token = Token::from_gh(run_gh).expect("read gh's token");
    let recorder = Recorder::new(Box::new(Https::new()), fixtures.clone());
    let github = GitHub::new(Box::new(recorder), token);
    let _ = github.viewer().expect("ask GitHub who the user is");
    record_first_sync(&github);
    println!("Recorded to {}", fixtures.display());
}

/// One page of each search, one batch of what they found, and one batch of the Issues those PRs name. Each
/// recording holds one answer, so more pages would only keep the last.
fn record_first_sync(github: &GitHub) {
    let repository = RepositoryName::parse(WORK_REPOSITORY).expect("a Repository's name");
    let mut ids = Vec::new();
    let first_page = PageRequest {
        size: SEARCH_PAGE,
        after: None,
    };
    for search in &WORK_SEARCHES {
        let page = github
            .search_pull_requests(search, &repository, first_page)
            .expect("search for the user's work");
        println!("{} IDs on the first page of a search", page.ids.len());
        for id in page.ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let batch = github
        .fetch_pull_requests(&ids, FETCH_BATCH)
        .expect("fetch the user's PRs");
    println!("{} PRs in the batch", batch.pull_requests.len());
    let mut named = BTreeSet::new();
    for pull_request in &batch.pull_requests {
        named.extend(possible_numbers(pull_request, &repository));
    }
    let numbers: Vec<u32> = named.into_iter().collect();
    if numbers.is_empty() {
        println!("Those PRs name no Issues, so FetchIssues.json stays as it was");
        return;
    }
    let issues = github
        .fetch_issues(&repository, &numbers, FETCH_BATCH)
        .expect("fetch the Issues the PRs name");
    println!(
        "{} Issues of {} numbers asked for",
        issues.issues.len(),
        issues.asked_for
    );
}
