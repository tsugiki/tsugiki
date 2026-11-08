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
mod tests;
