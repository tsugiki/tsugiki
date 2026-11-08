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
