//! Record mode (ADR 0009): `npm run record` asks real GitHub the client's read-only queries, with gh's
//! token, and writes the scrubbed answers to `tests/fixtures/github`. Read them before you commit them.

#![expect(
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a developer's command line: a failure should stop it and say why in the terminal"
)]

use std::collections::BTreeSet;
use std::path::Path;

use time::OffsetDateTime;
use tsugiki_core::error::Error;
use tsugiki_core::filters::Filter;
use tsugiki_core::github::GitHub;
use tsugiki_core::github::filters::{FilterHitKind, FilterSearch};
use tsugiki_core::github::issues::ListedPr;
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::github::pull_requests::{NodeId, PageRequest, PrFetch};
use tsugiki_core::github::record::Recorder;
use tsugiki_core::links::{possible_numbers, possible_origin};
use tsugiki_core::repository::{RepositoryAccess, RepositoryName};
use tsugiki_core::sync::{FETCH_BATCH, SEARCH_PAGE, WORK_SEARCHES};
use tsugiki_core::token::Token;

/// The Repository the first sync's recordings come from, which every signed-in user can read.
const WORK_REPOSITORY: &str = "python/cpython";
const ONLY_PAGE: PageRequest<'static> = PageRequest {
    size: SEARCH_PAGE,
    after: None,
};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let token = Token::from_gh(run_gh).expect("read gh's token");
    let scout = GitHub::new(
        Box::new(Https::new()),
        Token::from_gh(run_gh).expect("read gh's token"),
    );
    let recorder = Recorder::new(Box::new(Https::new()), fixtures.clone());
    let github = GitHub::new(Box::new(recorder), token);
    let _ = github.viewer().expect("ask GitHub who the user is");
    record_first_sync(&github, &scout);
    println!("Recorded to {}", fixtures.display());
}

/// One page of each search, one batch of what they found, one batch of the Issues those PRs name, and one of
/// those Issues' trees. Each recording holds one answer, so more pages would only keep the last.
fn record_first_sync(github: &GitHub, scout: &GitHub) {
    let repository = RepositoryName::parse(WORK_REPOSITORY).expect("a Repository's name");
    let _checked = github
        .check_repository(&repository, OffsetDateTime::now_utc().date())
        .expect("check the Repository");
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
    let mut named = BTreeSet::new();
    record_a_filter(github, &repository, &mut ids, &mut named);
    let batch = github
        .fetch_pull_requests(PrFetch::Work, &ids, FETCH_BATCH)
        .expect("fetch the user's PRs");
    println!("{} PRs in the batch", batch.pull_requests.len());
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
    let work: BTreeSet<u32> = batch.pull_requests.iter().map(|pr| pr.number).collect();
    let shown = issues.issues.iter().map(|issue| issue.number);
    let chosen = issue_with_a_backport(scout, &repository, shown.collect(), &work)
        .expect("look for an Issue with a Backport");
    let Some(issue) = chosen else {
        println!("No Issue has a Backport on one page, so the tree's recordings stay as they were");
        return;
    };
    record_a_tree(github, &repository, issue, &work).expect("record an Issue's tree");
}

/// The first page of the first starter Filter. Each Filter replays this one answer.
fn record_a_filter(
    github: &GitHub,
    repository: &RepositoryName,
    ids: &mut Vec<NodeId>,
    named: &mut BTreeSet<u32>,
) {
    let starters = Filter::starters(RepositoryAccess::Other);
    let starter = starters.first().expect("a starter Filter");
    let search = FilterSearch {
        query: &starter.query,
        created: None,
        page: ONLY_PAGE,
    };
    let page = github
        .search_filter(repository, &search)
        .expect("search a starter Filter");
    println!("{} items of {} in a Filter", page.hits.len(), page.total);
    for hit in page.hits {
        match hit.kind {
            FilterHitKind::Issue => drop(named.insert(hit.number)),
            FilterHitKind::PullRequest(id) => {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
}

/// The first Issue whose cross-references fit on one page and list a Backport. Replay answers every Issue
/// with one recorded page, so that page has to be the last, or a replayed sync would ask for the next one
/// forever. This asks through `scout`, which records nothing, so an Issue it passes over leaves no recording
/// behind.
fn issue_with_a_backport(
    scout: &GitHub,
    repository: &RepositoryName,
    shown: Vec<u32>,
    work: &BTreeSet<u32>,
) -> Result<Option<u32>, Error> {
    for issue in shown {
        let page = scout.fetch_cross_references(repository, issue, ONLY_PAGE)?;
        let ids = new_ids(page.listed, work);
        if page.next.is_some() || ids.is_empty() {
            continue;
        }
        let tree = scout.fetch_pull_requests(PrFetch::Tree, &ids, FETCH_BATCH)?;
        if tree
            .pull_requests
            .iter()
            .any(|pr| possible_origin(pr).is_some())
        {
            return Ok(Some(issue));
        }
    }
    Ok(None)
}

/// The IDs of the PRs in `listed` outside `work`, each once.
fn new_ids(listed: Vec<ListedPr>, work: &BTreeSet<u32>) -> Vec<NodeId> {
    let mut seen = work.clone();
    listed
        .into_iter()
        .filter(|pr| seen.insert(pr.number))
        .map(|pr| pr.id)
        .collect()
}

/// `issue`'s cross-references, the first batch of its tree and the Origin PRs that batch is missing.
fn record_a_tree(
    github: &GitHub,
    repository: &RepositoryName,
    issue: u32,
    work: &BTreeSet<u32>,
) -> Result<(), Error> {
    let page = github.fetch_cross_references(repository, issue, ONLY_PAGE)?;
    let ids = new_ids(page.listed, work);
    let tree = github.fetch_pull_requests(PrFetch::Tree, &ids, FETCH_BATCH)?;
    println!(
        "{} PRs in the tree of Issue {issue}",
        tree.pull_requests.len()
    );
    let origins: BTreeSet<u32> = tree
        .pull_requests
        .iter()
        .filter_map(possible_origin)
        .collect();
    let held: BTreeSet<u32> = tree.pull_requests.iter().map(|pr| pr.number).collect();
    for origin in origins
        .difference(&held)
        .filter(|number| !work.contains(number))
    {
        let answer = github.fetch_origin_pull_request(repository, *origin)?;
        println!(
            "Origin PR {origin}: found {}",
            answer.pull_request.is_some()
        );
    }
    Ok(())
}
