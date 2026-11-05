//! Fails a sync at each point it can fail and lets it retry. It has to end where a run with no failure ends,
//! having asked GitHub for the same things in the same order and told the window about the same items.
//!
//! The coverage gate counts a `?` as covered once its line has run, so it can't see an error path no test
//! takes. A subject's own test brings a scenario here, and these runs take each of its error paths.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use serde_json::{Value, json};

use super::testing::{Harness, cpython};
use super::{Progress, Syncs};
use crate::github::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::repository::RepositorySettings;
use crate::store::{ChangeCheck, Store, WorkFacts};
use crate::token::Token;

/// A request, by what would change its answer.
type Asked = (String, Value);

const MOST_STEPS: usize = 500;

/// A sync to fail, with what GitHub answers when nothing goes wrong.
pub(super) struct Scenario {
    pub(super) name: &'static str,
    pub(super) settings: RepositorySettings,
    /// Fills the store before the sync starts.
    pub(super) prepare: fn(&Harness),
    pub(super) posts: Vec<Result<Reply, Unreachable>>,
    pub(super) checks: Vec<ConditionalReply>,
}

fn asked_by_post(request: &Request<'_>) -> Asked {
    let sent = json!([request.query, request.variables]);
    (request.operation.to_owned(), sent)
}

fn asked_by_get(request: &ConditionalGet<'_>) -> Asked {
    (request.path.to_owned(), json!(request.etag))
}

/// Answers from the scenario's lists in order, and keeps each answer for the runs that fail.
struct Recording {
    posts: RefCell<VecDeque<Result<Reply, Unreachable>>>,
    checks: RefCell<VecDeque<ConditionalReply>>,
    posted: Sender<(Asked, Reply)>,
    checked: Sender<(Asked, ConditionalReply)>,
    served: Sender<Asked>,
}

impl Transport for Recording {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let asked = asked_by_post(request);
        let next = self.posts.borrow_mut().pop_front();
        let reply = next.expect("the scenario has a reply for each request")?;
        drop(self.posted.send((asked.clone(), reply.clone())));
        drop(self.served.send(asked));
        Ok(reply)
    }

    fn get(
        &self,
        _: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        let asked = asked_by_get(request);
        let next = self.checks.borrow_mut().pop_front();
        let reply = next.expect("the scenario has a reply for each list check");
        drop(self.checked.send((asked.clone(), reply.clone())));
        drop(self.served.send(asked));
        Ok(reply)
    }
}

/// Answers by what it's asked, because a step whose save failed asks again for what it already had. The
/// GraphQL requests in `failing` find GitHub unreachable. A failed list check stays out of this: it isn't a
/// failure, because the refresh then polls instead.
struct Replaying {
    posts: Vec<(Asked, Reply)>,
    checks: Vec<(Asked, ConditionalReply)>,
    failing: Range<usize>,
    asks: Cell<usize>,
    served: Sender<Asked>,
}

impl Replaying {
    fn serve<T: Clone>(&self, answers: &[(Asked, T)], asked: Asked) -> T {
        let known = answers.iter().find(|(request, _)| *request == asked);
        let (_, answer) = known.expect("a request the run with no failure didn't make");
        drop(self.served.send(asked));
        answer.clone()
    }
}

impl Transport for Replaying {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let at = self.asks.replace(self.asks.get() + 1);
        if self.failing.contains(&at) {
            return Err(Unreachable);
        }
        Ok(self.serve(&self.posts, asked_by_post(request)))
    }

    fn get(
        &self,
        _: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        Ok(self.serve(&self.checks, asked_by_get(request)))
    }
}

/// The end of a run, which each run that failed has to match.
#[derive(Debug, PartialEq, Eq)]
struct Ended {
    facts: WorkFacts,
    check: Option<ChangeCheck>,
    refresh_under_way: Option<bool>,
    told_the_window: BTreeSet<u32>,
}

/// A run's end, with the failures on the way there.
struct Run {
    ended: Ended,
    problems: Vec<String>,
}

/// Runs the sync to its end. A failure waits for no refresh here, so the next step is the retry.
fn run(settings: &RepositorySettings, harness: &Harness, store: &Store) -> Run {
    let mut syncs = Syncs::default();
    syncs.add(settings.clone());
    let mut told_the_window = BTreeSet::new();
    let mut steps = 0;
    let mut problems = Vec::new();
    while syncs.ready_at().is_some() {
        assert!(steps < MOST_STEPS, "the sync doesn't end");
        match syncs.step(&harness.github, store) {
            Ok(Some(Progress::Changed(change))) => told_the_window.extend(change.items),
            Ok(_) => {}
            Err(problem) => {
                syncs.after_failure(&problem, Instant::now());
                problems.push(problem.message);
            }
        }
        steps += 1;
    }
    let state = harness.store.sync_state(&cpython()).unwrap();
    let ended = Ended {
        facts: harness.reader.work_facts(&cpython()).unwrap(),
        check: state.as_ref().map(|state| state.check.clone()),
        refresh_under_way: state.map(|state| state.refresh_under_way),
        told_the_window,
    };
    Run { ended, problems }
}

/// A run that fails, and the requests it got answers to, with a repeat of one in a row counted once.
struct Failing<'a> {
    scenario: &'a Scenario,
    clean: &'a Ended,
    served: &'a [Asked],
    posts: &'a [(Asked, Reply)],
    checks: &'a [(Asked, ConditionalReply)],
}

impl Failing<'_> {
    /// Runs the sync with these failures, and answers whether any happened.
    fn check(&self, what: &str, failing: Range<usize>, broken: Option<usize>) -> bool {
        let (served, seen) = mpsc::channel();
        let transport = Replaying {
            posts: self.posts.to_vec(),
            checks: self.checks.to_vec(),
            failing,
            asks: Cell::new(0),
            served,
        };
        let name = format!("{}-{what}", self.scenario.name);
        let harness = Harness::answering(&name, Box::new(transport));
        (self.scenario.prepare)(&harness);

        let flaky = broken.map(|call| harness.store.failing_at_call(call));
        let store = flaky.as_ref().unwrap_or(&harness.store);
        let failed = run(&self.scenario.settings, &harness, store);
        if let Some(flaky) = flaky {
            flaky.close();
        }

        let mut asked: Vec<Asked> = seen.try_iter().collect();
        asked.dedup();
        assert_eq!(failed.ended, *self.clean, "{name}");
        assert_eq!(asked, self.served, "{name}");
        harness.finish();
        !failed.problems.is_empty()
    }
}

fn collected<T>(seen: &Receiver<T>) -> Vec<T> {
    seen.try_iter().collect()
}

/// Fails `scenario` at each request and at each call to the store. Answers with the operations a run with no
/// failure asks for, so the caller can say which subjects its scenario reaches.
pub(super) fn recovers_from_each_failure(scenario: &Scenario) -> Vec<String> {
    let (posted, posts) = mpsc::channel();
    let (checked, checks) = mpsc::channel();
    let (served, seen) = mpsc::channel();
    let transport = Recording {
        posts: RefCell::new(scenario.posts.iter().cloned().collect()),
        checks: RefCell::new(scenario.checks.iter().cloned().collect()),
        posted,
        checked,
        served,
    };
    let harness = Harness::answering(scenario.name, Box::new(transport));
    (scenario.prepare)(&harness);
    let clean = run(&scenario.settings, &harness, &harness.store);
    harness.finish();

    assert_eq!(
        clean.problems, [""; 0],
        "the scenario has to work with no failure"
    );
    let posts = collected(&posts);
    let checks = collected(&checks);
    let mut asked = collected(&seen);
    assert_eq!(posts.len(), scenario.posts.len(), "a reply went unasked");
    assert_eq!(checks.len(), scenario.checks.len(), "a check went unasked");
    asked.dedup();
    let failing = Failing {
        scenario,
        clean: &clean.ended,
        served: &asked,
        posts: &posts,
        checks: &checks,
    };
    for request in 0..posts.len() {
        // A request gets a second try at half the size, so a failure takes two.
        assert!(failing.check(&format!("ask-{request}"), request..request + 2, None));
    }
    let mut call = 0;
    while failing.check(&format!("call-{call}"), 0..0, Some(call)) {
        call += 1;
    }
    asked.into_iter().map(|(operation, _)| operation).collect()
}
