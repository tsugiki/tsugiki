use super::*;

fn line(json: &str) -> String {
    serde_json::from_str::<Miss>(json).unwrap().line()
}

#[test]
fn each_miss_says_what_was_slow_and_how_long_it_took() {
    let lines = [
        (
            r#"{"kind": "startup", "ms": 812.4}"#,
            "Start-up to a usable Workbench took 812 ms",
        ),
        (
            r#"{"kind": "view_query", "ms": 73.25}"#,
            "Slow view query: 73.2 ms",
        ),
        (
            r#"{"kind": "issue_from_database", "number": 1234, "ms": 240.06}"#,
            "Slow opening of #1234 from the database: 240.1 ms",
        ),
        (
            r#"{"kind": "issue_from_github", "number": 1234, "ms": 2400.4}"#,
            "Slow opening of #1234 from GitHub: 2400 ms",
        ),
        (
            r#"{"kind": "jump", "ms": 140.25}"#,
            "Slow jump in a diff: 140.2 ms to its rows",
        ),
        (
            r#"{"kind": "input", "input": "key", "ms": 41}"#,
            "Slow key: 41.0 ms to the next paint",
        ),
        (
            r#"{"kind": "input", "input": "click", "ms": 33}"#,
            "Slow click: 33.0 ms to the next paint",
        ),
    ];
    for (json, expected) in lines {
        assert_eq!(line(json), expected);
    }
}

#[test]
fn a_long_frame_names_its_scripts() {
    let json = r#"{"kind": "long_frame", "ms": 82, "scripts": [
            {"invoker": "HTMLDivElement.onkeydown", "function": "onKey", "source": "/_app/start.js", "ms": 60},
            {"invoker": "FrameRequestCallback", "function": null, "source": null, "ms": 12.5}]}"#;
    assert_eq!(
        line(json),
        "Long frame: 82.0 ms, 60.0 ms from HTMLDivElement.onkeydown in onKey at /_app/start.js; \
             12.5 ms from FrameRequestCallback"
    );
}

#[test]
fn a_long_frame_with_no_script_says_so() {
    assert_eq!(
        line(r#"{"kind": "long_frame", "ms": 55, "scripts": []}"#),
        "Long frame: 55.0 ms, no script known"
    );
}

#[test]
fn a_cap_says_which_misses_stop_being_logged() {
    let lines = [
        ("view_query", "slow view queries"),
        ("issue_from_database", "slow openings from the database"),
        ("issue_from_github", "slow openings from GitHub"),
        ("jump", "slow jumps in a diff"),
        ("input", "slow keys and clicks"),
        ("long_frame", "long frames"),
    ];
    for (of, misses) in lines {
        assert_eq!(
            line(&format!(
                r#"{{"kind": "capped", "of": "{of}", "limit": 10}}"#
            )),
            format!("10 {misses} in a minute, so no more are logged until it ends")
        );
    }
}
