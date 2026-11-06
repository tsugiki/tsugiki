//! Fails a sync at each point it can fail and lets it retry. It has to end where a run with no failure ends,
//! having asked GitHub for the same things in the same order and told the window about the same items.
//!
//! The coverage gate counts a `?` as covered once its line has run, so it can't see an error path no test
//! takes. A subject's own test brings a scenario here, and these runs take each of its error paths.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use serde_json::{Value, json};

use super::testing::Harness;
use super::{Progress, Syncs};
use crate::github::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::repository::RepositorySettings;
use crate::store::{ActivityFacts, ChangeCheck, Store, WorkFacts};
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

/// The window's edge comes from the clock, so each run has its own, and it doesn't say what's asked for.
fn asked_by_post(request: &Request<'_>) -> Asked {
    let mut variables = request.variables.clone();
    if request.operation == "WindowIssues" {
        variables["since"] = Value::Null;
    }
    let sent = json!([request.query, variables]);
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
    /// Each thing the window heard, which may name no item, as for the Sidebar.
    told_the_window: Vec<Vec<u32>>,
}

/// A run's end, with the failures on the way there.
struct Run {
    ended: Ended,
    problems: Vec<String>,
    steps: usize,
}

/// A way for a run to go wrong.
struct Trouble {
    /// The requests GitHub doesn't answer.
    failing: Range<usize>,
    /// The call to the store that goes missing.
    lost: Option<usize>,
    /// The step before which the user quits, so that a new queue takes the sync up from the store.
    quit_before: Option<usize>,
}

/// Runs the sync to its end. A failure waits for no refresh here, so the next step is the retry.
fn run(
    settings: &RepositorySettings,
    harness: &Harness,
    store: &Store,
    quit_before: Option<usize>,
) -> Run {
    let mut syncs = Syncs::default();
    syncs.add(settings.clone());
    let mut told_the_window = Vec::new();
    let mut steps = 0;
    let mut problems = Vec::new();
    while syncs.ready_at().is_some() {
        assert!(steps < MOST_STEPS, "the sync doesn't end");
        if quit_before == Some(steps) {
            syncs = Syncs::default();
            syncs.add(settings.clone());
        }
        match syncs.step(&harness.github, store) {
            Ok(Some(Progress::Changed(change))) => told_the_window.push(change.items),
            Ok(_) => {}
            Err(problem) => {
                syncs.after_failure(&problem, Instant::now(), harness.github.clock());
                problems.push(problem.message);
            }
        }
        steps += 1;
    }
    let state = harness.store.sync_state(&settings.name).unwrap();
    let ended = Ended {
        // A retried save notices its events and CI results a little later, which nothing compares.
        facts: WorkFacts {
            activity: ActivityFacts::default(),
            ..harness.reader.work_facts(&settings.name).unwrap()
        },
        check: state.as_ref().map(|state| state.check.clone()),
        refresh_under_way: state.map(|state| state.refresh_under_way),
        told_the_window,
    };
    Run {
        ended,
        problems,
        steps,
    }
}

/// Takes out the repeats in a row, of which a run can have `allowed`.
fn once_each(mut asked: Vec<Asked>, allowed: usize) -> Vec<Asked> {
    let all = asked.len();
    asked.dedup();
    assert!(all - asked.len() <= allowed, "a step repeated itself");
    asked
}

/// The run with no failure, which each run that fails has to match.
struct Failing<'a> {
    scenario: &'a Scenario,
    clean: &'a Ended,
    served: &'a [Asked],
    posts: &'a [(Asked, Reply)],
    checks: &'a [(Asked, ConditionalReply)],
}

impl Failing<'_> {
    /// Runs the sync with this trouble, and answers with the failures it saw. A step whose save failed may
    /// ask once more for what it already had. A request that failed has no answer to repeat, and a sync that
    /// quit comes back to the next page.
    fn check(&self, what: &str, trouble: Trouble) -> Vec<String> {
        let Trouble {
            failing,
            lost,
            quit_before,
        } = trouble;
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

        let (flaky, _calls) = harness.store.failing_at_call(lost);
        let failed = run(&self.scenario.settings, &harness, &flaky, quit_before);
        flaky.close();

        let asked = once_each(seen.try_iter().collect(), usize::from(lost.is_some()));
        assert_eq!(failed.ended, *self.clean, "{name}");
        assert_eq!(asked, self.served, "{name}");
        harness.finish();
        failed.problems
    }
}

fn collected<T>(seen: &Receiver<T>) -> Vec<T> {
    seen.try_iter().collect()
}

/// The record of a run with no failure.
struct Clean {
    run: Run,
    asked: Vec<Asked>,
    posts: Vec<(Asked, Reply)>,
    checks: Vec<(Asked, ConditionalReply)>,
    store_calls: usize,
}

impl Clean {
    fn of(scenario: &Scenario) -> Self {
        run_clean(scenario)
    }

    fn failing<'a>(&'a self, scenario: &'a Scenario) -> Failing<'a> {
        Failing {
            scenario,
            clean: &self.run.ended,
            served: &self.asked,
            posts: &self.posts,
            checks: &self.checks,
        }
    }
}

fn run_clean(scenario: &Scenario) -> Clean {
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
    let (counted, calls) = harness.store.failing_at_call(None);
    let clean = run(&scenario.settings, &harness, &counted, None);
    counted.close();
    harness.finish();

    assert_eq!(
        clean.problems, [""; 0],
        "the scenario has to work with no failure"
    );
    let posts = collected(&posts);
    let checks = collected(&checks);
    let asked = once_each(collected(&seen), 0);
    assert_eq!(posts.len(), scenario.posts.len(), "a reply went unasked");
    assert_eq!(checks.len(), scenario.checks.len(), "a check went unasked");
    Clean {
        run: clean,
        asked,
        posts,
        checks,
        store_calls: calls.try_iter().count(),
    }
}

/// Fails `scenario` at each request and at each call to the store. Answers with the operations a run with no
/// failure asks for, so the caller can say which subjects its scenario reaches.
pub(super) fn recovers_from_each_failure(scenario: &Scenario) -> Vec<String> {
    let clean = Clean::of(scenario);
    let failing = clean.failing(scenario);
    for request in 0..clean.posts.len() {
        // A request gets a second try at half the size, so a failure takes two.
        let trouble = Trouble {
            failing: request..request + 2,
            lost: None,
            quit_before: None,
        };
        assert!(!failing.check(&format!("ask-{request}"), trouble).is_empty());
    }
    for call in 0..clean.store_calls {
        let trouble = Trouble {
            failing: 0..0,
            lost: Some(call),
            quit_before: None,
        };
        drop(failing.check(&format!("call-{call}"), trouble));
    }
    let asked = clean.asked.iter();
    asked.map(|(operation, _)| operation.clone()).collect()
}

/// Quits a first sync before each of its steps, and takes it up again from the store. It has to end the same,
/// and to ask GitHub for each thing once.
pub(super) fn resumes_after_each_step(scenario: &Scenario) {
    let clean = Clean::of(scenario);
    let failing = clean.failing(scenario);
    for step in 0..clean.run.steps {
        let trouble = Trouble {
            failing: 0..0,
            lost: None,
            quit_before: Some(step),
        };
        assert_eq!(failing.check(&format!("quit-{step}"), trouble), [""; 0]);
    }
}
