use super::*;

#[test]
fn a_full_recording_says_each_number_with_the_version() {
    let measurement = Measurement {
        seconds: 42.34,
        frames: Some(Frames {
            count: 2530,
            p95_ms: 12.06,
            worst_ms: 31.0,
            limit_ms: 25.0,
            over_limit: 2,
        }),
        startup_ms: Some(812.4),
        keys: Some(Slowest {
            count: 14,
            worst_ms: 9.04,
        }),
        view_queries: Some(Slowest {
            count: 1,
            worst_ms: 31.25,
        }),
        refreshes: Some(Refreshes {
            count: 1,
            dropped_frames: 0,
        }),
        issues: Some(IssueOpenings {
            from_database: Slowest {
                count: 5,
                worst_ms: 12.04,
            },
            from_github: Some(Slowest {
                count: 2,
                worst_ms: 640.0,
            }),
        }),
        jumps: Some(Jumps {
            rows: Slowest {
                count: 3,
                worst_ms: 61.04,
            },
            plain: 1,
        }),
        long_frames: Vec::new(),
    };

    assert_eq!(
        measurement.summary(),
        format!(
            "Performance of Tsugiki {} over 42.3 s: frame p95 12.1 ms, worst frame 31.0 ms, 2 of 2530 \
                 frames over 25 ms; start-up 812 ms; worst key to frame 9.0 ms of 14 keys; worst view query \
                 31.2 ms of 1 query; 0 frames dropped in 1 refresh; slowest Issue from the database 12.0 ms of \
                 5 Issues, from GitHub 640.0 ms of 2; slowest jump in a diff 61.0 ms of 3 jumps, 1 plain",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn a_recording_that_saw_nothing_says_so() {
    let measurement = Measurement {
        seconds: 0.5,
        frames: None,
        startup_ms: None,
        keys: None,
        view_queries: None,
        refreshes: None,
        issues: None,
        jumps: None,
        long_frames: Vec::new(),
    };

    let summary = measurement.summary();
    assert!(
            summary.ends_with(
                "over 0.5 s: no frames; start-up not seen; no keys; no view queries; no refresh; no Issues \
                 opened; no jumps in a diff"
            ),
            "{summary}"
        );
}

#[test]
fn the_window_sends_it_as_json() {
    let sent = r#"{"seconds": 1.5, "frames": null, "startup_ms": 700, "keys": null,
            "view_queries": {"count": 2, "worst_ms": 20.5}, "refreshes": {"count": 2, "dropped_frames": 1},
            "issues": {"from_database": {"count": 1, "worst_ms": 8}, "from_github": null},
            "long_frames": [{"at_s": 1.25, "ms": 40.1, "cause": {"kind": "issue_text", "number": 1234}}]}"#;

    let measurement: Measurement = serde_json::from_str(sent).unwrap();

    let summary = measurement.summary();
    assert!(
            summary.ends_with(
                "1 frame dropped in 2 refreshes; slowest Issue from the database 8.0 ms of 1 Issue, none \
                 from GitHub; no jumps in a diff"
            ),
            "{summary}"
        );
    assert_eq!(
        measurement.long_frame_lines(),
        ["Long frame at 1.2 s: 40.1 ms, after #1234's text arrived from GitHub"]
    );
}

#[test]
fn each_long_frame_gets_a_line() {
    let frame = |at_s, cause| LongFrame {
        at_s,
        ms: 30.0,
        cause,
    };
    let measurement = Measurement {
        seconds: 10.0,
        frames: None,
        startup_ms: None,
        keys: None,
        view_queries: None,
        refreshes: None,
        issues: None,
        jumps: None,
        long_frames: vec![
            frame(1.0, Some(Cause::Key)),
            frame(2.0, Some(Cause::Click)),
            frame(3.0, Some(Cause::ViewQuery)),
            frame(4.0, None),
        ],
    };

    assert_eq!(
        measurement.long_frame_lines(),
        [
            "Long frame at 1.0 s: 30.0 ms, after a key",
            "Long frame at 2.0 s: 30.0 ms, after a click",
            "Long frame at 3.0 s: 30.0 ms, after a view's answer",
            "Long frame at 4.0 s: 30.0 ms, with no known cause",
        ]
    );
}

#[test]
fn the_log_lists_the_first_hundred_long_frames_and_counts_the_rest() {
    let frames = |count| {
        (0..count)
            .map(|at| LongFrame {
                at_s: f64::from(at),
                ms: 30.0,
                cause: None,
            })
            .collect()
    };
    let mut measurement = Measurement {
        seconds: 600.0,
        frames: None,
        startup_ms: None,
        keys: None,
        view_queries: None,
        refreshes: None,
        issues: None,
        jumps: None,
        long_frames: frames(101),
    };

    let lines = measurement.long_frame_lines();
    assert_eq!(lines.len(), 101);
    assert_eq!(
        lines[99],
        "Long frame at 99.0 s: 30.0 ms, with no known cause"
    );
    assert_eq!(lines[100], "1 more long frame, not listed");

    measurement.long_frames = frames(103);
    assert_eq!(
        measurement.long_frame_lines()[100],
        "3 more long frames, not listed"
    );
}
