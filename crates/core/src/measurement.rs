//! A recording of the performance HUD (ADR 0017), and the lines it writes to `tsugiki.log`. The maintainer
//! pastes the summary line into a release's notes, so the next release can be compared with it, and the
//! lines after it say what came before each long frame.

use serde::Deserialize;
use specta::Type;

/// One recording, as the window sends it. Each time is in milliseconds.
#[derive(Debug, Clone, PartialEq, Deserialize, Type)]
pub struct Measurement {
    pub seconds: f64,
    /// Nothing, if the window drew no frame while recording.
    pub frames: Option<Frames>,
    /// From Tsugiki's code starting to run to a usable Workbench, if this run opened on one.
    pub startup_ms: Option<f64>,
    /// From each key to the frame after it.
    pub keys: Option<Slowest>,
    /// The window's wait for each view query.
    pub view_queries: Option<Slowest>,
    pub refreshes: Option<Refreshes>,
    /// The Issues opened while recording, if any.
    pub issues: Option<IssueOpenings>,
    /// The jumps in a PR's diff while recording, if any.
    pub jumps: Option<Jumps>,
    /// Each frame over the limit, in the order they came.
    pub long_frames: Vec<LongFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct Frames {
    pub count: u32,
    pub p95_ms: f64,
    pub worst_ms: f64,
    /// The longest a frame may take while the window scrolls.
    pub limit_ms: f64,
    pub over_limit: u32,
}

/// The slowest of `count` timings.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct Slowest {
    pub count: u32,
    pub worst_ms: f64,
}

/// The background refreshes that ran while the window drew frames.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct Refreshes {
    pub count: u32,
    pub dropped_frames: u32,
}

/// The Issues opened while recording, from the click to the Issue on screen.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct IssueOpenings {
    /// Each Issue, until the window showed what the database held.
    pub from_database: Slowest,
    /// The ones whose text the database didn't hold, until that text arrived from GitHub.
    pub from_github: Option<Slowest>,
}

/// The jumps in a PR's diff while recording: each scroll that showed rows the window didn't hold, until
/// they were on screen (#180).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct Jumps {
    pub rows: Slowest,
    /// How many showed their rows plain, as their spans hadn't come in time.
    pub plain: u32,
}

/// The most long frames the log lists from one recording, so the summary line stays easy to find.
const LONG_FRAMES_LISTED: usize = 100;

/// A frame over the limit.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Type)]
pub struct LongFrame {
    /// Its end, in seconds from the start of the recording.
    pub at_s: f64,
    pub ms: f64,
    /// Nothing, if Tsugiki did nothing the HUD knows of during it or right before.
    pub cause: Option<Cause>,
}

/// The last thing that happened before a long frame. A key isn't named, so the log holds nothing the
/// user typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    Key,
    Click,
    ViewQuery,
    /// An Issue's description and comments arrived from GitHub.
    IssueText {
        number: u32,
    },
}

impl Measurement {
    /// A line for each long frame, up to `LONG_FRAMES_LISTED`, and then how many more there were.
    pub fn long_frame_lines(&self) -> Vec<String> {
        let listed = self
            .long_frames
            .iter()
            .take(LONG_FRAMES_LISTED)
            .map(|frame| {
                let cause = match frame.cause {
                    None => "with no known cause".to_owned(),
                    Some(Cause::Key) => "after a key".to_owned(),
                    Some(Cause::Click) => "after a click".to_owned(),
                    Some(Cause::ViewQuery) => "after a view's answer".to_owned(),
                    Some(Cause::IssueText { number }) => {
                        format!("after #{number}'s text arrived from GitHub")
                    }
                };
                format!(
                    "Long frame at {:.1} s: {:.1} ms, {cause}",
                    frame.at_s, frame.ms
                )
            });
        let unlisted = self.long_frames.len().saturating_sub(LONG_FRAMES_LISTED);
        let more = (unlisted > 0).then(|| {
            let frames = if unlisted == 1 { "frame" } else { "frames" };
            format!("{unlisted} more long {frames}, not listed")
        });
        listed.chain(more).collect()
    }

    /// The summary line, with this build's version.
    pub fn summary(&self) -> String {
        let frames = self.frames.map_or_else(
            || "no frames".to_owned(),
            |frames| {
                format!(
                    "frame p95 {:.1} ms, worst frame {:.1} ms, {} of {} over {} ms",
                    frames.p95_ms,
                    frames.worst_ms,
                    frames.over_limit,
                    counted(frames.count, "frame", "frames"),
                    frames.limit_ms,
                )
            },
        );
        let startup = self.startup_ms.map_or_else(
            || "start-up not seen".to_owned(),
            |ms| format!("start-up {ms:.0} ms"),
        );
        let keys = self.keys.map_or_else(
            || "no keys".to_owned(),
            |keys| {
                format!(
                    "worst key to frame {:.1} ms of {}",
                    keys.worst_ms,
                    counted(keys.count, "key", "keys")
                )
            },
        );
        let queries = self.view_queries.map_or_else(
            || "no view queries".to_owned(),
            |queries| {
                format!(
                    "worst view query {:.1} ms of {}",
                    queries.worst_ms,
                    counted(queries.count, "query", "queries")
                )
            },
        );
        let refreshes = self.refreshes.map_or_else(
            || "no refresh".to_owned(),
            |refreshes| {
                format!(
                    "{} dropped in {}",
                    counted(refreshes.dropped_frames, "frame", "frames"),
                    counted(refreshes.count, "refresh", "refreshes")
                )
            },
        );
        let issues = self.issues.map_or_else(
            || "no Issues opened".to_owned(),
            |issues| {
                let github = issues.from_github.map_or_else(
                    || "none from GitHub".to_owned(),
                    |github| format!("from GitHub {:.1} ms of {}", github.worst_ms, github.count),
                );
                format!(
                    "slowest Issue from the database {:.1} ms of {}, {github}",
                    issues.from_database.worst_ms,
                    counted(issues.from_database.count, "Issue", "Issues")
                )
            },
        );
        let jumps = self.jumps.map_or_else(
            || "no jumps in a diff".to_owned(),
            |jumps| {
                format!(
                    "slowest jump in a diff {:.1} ms of {}, {} plain",
                    jumps.rows.worst_ms,
                    counted(jumps.rows.count, "jump", "jumps"),
                    jumps.plain
                )
            },
        );
        format!(
            "Performance of Tsugiki {} over {:.1} s: {}",
            env!("CARGO_PKG_VERSION"),
            self.seconds,
            [frames, startup, keys, queries, refreshes, issues, jumps].join("; ")
        )
    }
}

fn counted(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
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
}
