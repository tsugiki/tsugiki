//! A recording of the performance HUD (ADR 0017), and the one line it writes to `tsugiki.log`. The
//! maintainer pastes the line into a release's notes, so the next release can be compared with it.

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

impl Measurement {
    /// The line for the log, with this build's version.
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
        format!(
            "Performance of Tsugiki {} over {:.1} s: {}",
            env!("CARGO_PKG_VERSION"),
            self.seconds,
            [frames, startup, keys, queries, refreshes].join("; ")
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
        };

        assert_eq!(
            measurement.summary(),
            format!(
                "Performance of Tsugiki {} over 42.3 s: frame p95 12.1 ms, worst frame 31.0 ms, 2 of 2530 \
                 frames over 25 ms; start-up 812 ms; worst key to frame 9.0 ms of 14 keys; worst view query \
                 31.2 ms of 1 query; 0 frames dropped in 1 refresh",
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
        };

        let summary = measurement.summary();
        assert!(
            summary.ends_with(
                "over 0.5 s: no frames; start-up not seen; no keys; no view queries; no refresh"
            ),
            "{summary}"
        );
    }

    #[test]
    fn the_window_sends_it_as_json() {
        let sent = r#"{"seconds": 1.5, "frames": null, "startup_ms": 700, "keys": null,
            "view_queries": {"count": 2, "worst_ms": 20.5}, "refreshes": {"count": 2, "dropped_frames": 1}}"#;

        let measurement: Measurement = serde_json::from_str(sent).unwrap();

        let summary = measurement.summary();
        assert!(
            summary
                .ends_with("worst view query 20.5 ms of 2 queries; 1 frame dropped in 2 refreshes"),
            "{summary}"
        );
    }
}
