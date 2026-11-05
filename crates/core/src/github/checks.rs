//! The checks that failed on a PR. A CPython PR runs some 70, too many to read with every fetch, so Tsugiki
//! asks only about the user's own PRs whose CI failed.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::pull_requests::{Nodes, RateLimit};
use super::{GitHub, Request};
use crate::error::Error;
use crate::repository::RepositoryName;

const FETCH_FAILED_CHECKS: &str = "FetchFailedChecks";
/// More checks than GitHub answers at a time. A PR with more keeps the names on this page.
const FIELDS: &str = "lastCommit: commits(last: 1) { nodes { commit { statusCheckRollup { \
     contexts(first: 100) { nodes { __typename \
     ... on CheckRun { name conclusion checkSuite { workflowRun { workflow { name } } } } \
     ... on StatusContext { context state } } } } } } }";
/// The end of a check run, as far as a rollup goes.
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ConclusionWire {
    Failure,
    TimedOut,
    Cancelled,
    StartupFailure,
    ActionRequired,
    /// It passed, or GitHub skipped it.
    #[serde(other)]
    Harmless,
}

/// A commit status's state, as far as a rollup goes.
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StatusStateWire {
    Failure,
    Error,
    /// It passed, or it's still running.
    #[serde(other)]
    Harmless,
}

#[derive(Deserialize)]
struct NamedWire {
    name: String,
}

#[derive(Deserialize)]
struct WorkflowRunWire {
    workflow: NamedWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckSuiteWire {
    workflow_run: Option<WorkflowRunWire>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
enum ContextWire {
    CheckRun {
        name: String,
        conclusion: Option<ConclusionWire>,
        check_suite: Option<CheckSuiteWire>,
    },
    StatusContext {
        context: String,
        state: StatusStateWire,
    },
}

impl ContextWire {
    /// The name people know this check by, if it failed: "Tests / Ubuntu" for a job of a workflow.
    fn failed_name(self) -> Option<String> {
        match self {
            Self::CheckRun {
                name,
                conclusion,
                check_suite,
            } => {
                if matches!(conclusion?, ConclusionWire::Harmless) {
                    return None;
                }
                let run = check_suite.and_then(|suite| suite.workflow_run);
                Some(match run {
                    Some(run) => format!("{} / {name}", run.workflow.name),
                    None => name,
                })
            }
            Self::StatusContext { context, state } => {
                (!matches!(state, StatusStateWire::Harmless)).then_some(context)
            }
        }
    }
}

#[derive(Deserialize)]
struct RollupWire {
    contexts: Nodes<ContextWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitWire {
    status_check_rollup: Option<RollupWire>,
}

#[derive(Deserialize)]
struct LastCommitWire {
    commit: CommitWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestWire {
    last_commit: Nodes<LastCommitWire>,
}

impl PullRequestWire {
    /// The failed checks of its last commit by name, each once.
    fn failed_names(self) -> Vec<String> {
        let commits = self.last_commit.nodes.into_iter().flatten();
        let rollups = commits.filter_map(|last| last.commit.status_check_rollup);
        let contexts = rollups.flat_map(|rollup| rollup.contexts.nodes.into_iter().flatten());
        let mut names: Vec<String> = contexts.filter_map(ContextWire::failed_name).collect();
        names.sort();
        names.dedup();
        names
    }
}

/// One answer to `failed_checks`.
#[derive(Debug, PartialEq, Eq)]
pub struct FailedChecks {
    /// Each PR it asked about, with the names of the checks that failed. A PR GitHub no longer has names
    /// none, so nobody asks about it again.
    pub found: Vec<(u32, Vec<String>)>,
    pub rate_limit: RateLimit,
}

fn failed_checks_query(numbers: &[u32]) -> String {
    let fields: Vec<String> = numbers
        .iter()
        .map(|number| format!("p{number}: pullRequest(number: {number}) {{ {FIELDS} }}"))
        .collect();
    format!(
        "query {FETCH_FAILED_CHECKS}($owner: String!, $name: String!) {{ \
         repository(owner: $owner, name: $name) {{ {} }} rateLimit {{ remaining resetAt }} }}",
        fields.join(" ")
    )
}

impl GitHub {
    /// The failed checks of the first `size` of `numbers`, or half that after a 502 or no answer. `numbers`
    /// holds at least one.
    pub fn failed_checks(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<FailedChecks, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: BTreeMap<String, Option<PullRequestWire>>,
            rate_limit: RateLimit,
        }
        let request = |size: usize| {
            let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
            Request {
                operation: FETCH_FAILED_CHECKS,
                query: failed_checks_query(&asked).into(),
                variables: json!({ "owner": repository.owner(), "name": repository.name() }),
            }
        };
        let (mut data, asked_for): (Data, usize) =
            self.query_at_size(size.min(numbers.len()), request)?;
        let asked = numbers.iter().take(asked_for);
        let found = asked.map(|number| {
            let answer = data.repository.remove(&format!("p{number}")).flatten();
            let names = answer.map_or_else(Vec::new, PullRequestWire::failed_names);
            (*number, names)
        });
        Ok(FailedChecks {
            found: found.collect(),
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::github::testing::{Scripted, reply_ok, token};

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn run(name: &str, conclusion: Option<&str>, workflow: Option<&str>) -> Value {
        let run = workflow.map(|name| json!({ "workflow": { "name": name } }));
        json!({
            "__typename": "CheckRun",
            "name": name,
            "conclusion": conclusion,
            "checkSuite": { "workflowRun": run },
        })
    }

    fn status(context: &str, state: &str) -> Value {
        json!({ "__typename": "StatusContext", "context": context, "state": state })
    }

    /// A `GitHub` that gives one `answer`.
    fn answering(data: &Value) -> GitHub {
        let mut data = data.clone();
        data["rateLimit"] = json!({ "remaining": 4000, "resetAt": "2026-11-04T12:00:00Z" });
        let reply = reply_ok(&json!({ "data": data }).to_string());
        let (script, _asked) = Scripted::new(vec![reply]);
        GitHub::new(Box::new(script), token())
    }

    fn answer(contexts: &[Value]) -> Value {
        json!({ "repository": { "p1": {
            "number": 1,
            "lastCommit": { "nodes": [{ "commit": { "statusCheckRollup": {
                "contexts": { "nodes": contexts },
            } } }] },
        } } })
    }

    fn names_in(contexts: &[Value]) -> Vec<String> {
        let github = answering(&answer(contexts));
        let read = github.failed_checks(&cpython(), &[1], 50).unwrap();
        assert_eq!(read.found.len(), 1);
        read.found[0].1.clone()
    }

    #[test]
    fn a_failed_job_is_named_by_its_workflow_and_a_status_by_its_context() {
        let names = names_in(&[
            run("Ubuntu", Some("FAILURE"), Some("Tests")),
            run("Docs", Some("TIMED_OUT"), None),
            run("Lint", Some("CANCELLED"), None),
            run("Setup", Some("STARTUP_FAILURE"), None),
            run("Deploy", Some("ACTION_REQUIRED"), None),
            status("netlify", "ERROR"),
            status("readthedocs", "FAILURE"),
        ]);

        assert_eq!(
            names,
            [
                "Deploy",
                "Docs",
                "Lint",
                "Setup",
                "Tests / Ubuntu",
                "netlify",
                "readthedocs"
            ]
        );
    }

    #[test]
    fn what_passed_is_still_running_or_was_skipped_is_left_out() {
        let names = names_in(&[
            run("Ubuntu", Some("SUCCESS"), Some("Tests")),
            run("Windows", None, Some("Tests")),
            run("macOS", Some("SKIPPED"), Some("Tests")),
            status("netlify", "SUCCESS"),
        ]);

        assert_eq!(names, [""; 0]);
    }

    #[test]
    fn a_job_that_failed_on_several_triggers_is_named_once() {
        let failed = run("Ubuntu", Some("FAILURE"), Some("Tests"));

        assert_eq!(names_in(&[failed.clone(), failed]), ["Tests / Ubuntu"]);
    }

    #[test]
    fn a_pr_with_no_rollup_and_a_pr_github_no_longer_has_name_no_checks() {
        let answer = json!({ "repository": {
            "p1": {
                "number": 1,
                "lastCommit": { "nodes": [{ "commit": { "statusCheckRollup": null } }] },
            },
            "p2": null,
        } });
        let github = answering(&answer);

        let read = github.failed_checks(&cpython(), &[1, 2], 50).unwrap();

        assert_eq!(read.found, [(1, vec![]), (2, vec![])]);
    }
}
