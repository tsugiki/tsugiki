use super::*;
use crate::github::{Request, Transport};
use crate::token::{GhOutput, Token};

fn fixture(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = crate::testing::temp_path(&format!("replay-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Op.json"), contents).unwrap();
    dir
}

fn token() -> Token {
    Token::from_gh(|_| {
        Ok(GhOutput {
            success: true,
            stdout: "t".to_owned(),
        })
    })
    .unwrap()
}

/// A read by number of `numbers`, each field asking for `fields`.
fn numbered(numbers: &[u32], fields: &str) -> String {
    let asked: Vec<String> = numbers
        .iter()
        .map(|n| format!("i{n}: issueOrPullRequest(number: {n}) {{ {fields} }}"))
        .collect();
    format!(
        "query Op {{ repository {{ {} }} rateLimit {{ remaining }} }}",
        asked.join(" ")
    )
}

/// A replay's answer to `query`, with `recorded` as the recording of `operation`.
#[expect(
    clippy::unwrap_in_result,
    reason = "a test helper: the `Result` is whether the replay panicked"
)]
fn replayed(
    name: &str,
    operation: &str,
    recorded: &str,
    query: String,
) -> std::thread::Result<Reply> {
    let text = serde_json::json!({ "query": recorded, "status": 200, "reset": null, "body": {} });
    let dir = fixture(name, &text.to_string());
    std::fs::rename(dir.join("Op.json"), dir.join(format!("{operation}.json"))).unwrap();
    let request = Request {
        operation,
        query: query.into(),
        variables: serde_json::Value::Null,
    };
    let replayer = Replayer::new(dir.clone());
    let token = token();
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        replayer.post(&token, &request).unwrap()
    }));
    std::fs::remove_dir_all(dir).unwrap();
    answer
}

#[test]
fn a_read_of_numbers_no_recording_holds_finds_none() {
    for operation in ["FetchIssues", "FetchIssueReferences"] {
        let recorded = numbered(&[7, 8, 9], "number");
        let name = format!("other-{operation}");
        let reply = replayed(&name, operation, &recorded, numbered(&[120], "number")).unwrap();
        assert_eq!(reply.body, NO_ISSUES);
    }
}

#[test]
fn a_read_by_number_that_asks_for_other_fields_needs_a_new_recording() {
    let recorded = numbered(&[7, 8], "number");
    let asked = numbered(&[7, 8], "number title");
    assert!(replayed("changed-fields", "FetchIssues", &recorded, asked).is_err());
}

#[test]
fn another_operation_with_other_numbers_needs_a_new_recording() {
    let recorded = numbered(&[7], "number");
    assert!(replayed("other-operation", "Op", &recorded, numbered(&[8], "number")).is_err());
}

#[test]
fn a_changed_query_needs_a_new_recording() {
    let dir = fixture(
        "stale",
        r#"{"query":"old","status":200,"reset":null,"body":{}}"#,
    );
    let request = Request {
        operation: "Op",
        query: "new".into(),
        variables: serde_json::Value::Null,
    };
    let replayer = Replayer::new(dir.clone());
    let token = token();
    // Caught rather than `should_panic`, so the test can still remove its folder.
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        replayer.post(&token, &request)
    }))
    .unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains("record it again"), "{message}");
}
