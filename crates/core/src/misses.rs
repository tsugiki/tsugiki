//! The budgets of ADR 0017 that the window misses in daily use, which it logs through 0.x whether or not the
//! performance HUD shows. The log then says where Tsugiki is slow without anyone starting a recording.

use serde::Deserialize;
use specta::Type;

/// One budget the window missed, or the start-up time, which it logs each launch. Each time is in
/// milliseconds.
#[derive(Debug, Clone, PartialEq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Miss {
    /// From Tsugiki's code starting to run to a usable Workbench, slow or not.
    Startup { ms: f64 },
    /// The window's wait for a view query.
    ViewQuery { ms: f64 },
    /// From the click on an Issue to the window showing what the database held of it.
    IssueFromDatabase { number: u32, ms: f64 },
    /// From the click on an Issue to its text arriving from GitHub.
    IssueFromGithub { number: u32, ms: f64 },
    /// From a scroll in a PR's diff to the rows it hadn't held on screen (#180).
    Jump { ms: f64 },
    /// From a key or click to the next paint.
    Input { input: Input, ms: f64 },
    /// A frame the browser reports as long, with the scripts that ran in it, longest first.
    LongFrame { ms: f64, scripts: Vec<Script> },
    /// The window logged `limit` misses of one kind within a minute, and logs no more of them until it ends.
    Capped { of: MissKind, limit: u32 },
}

/// A key or a click. The key itself isn't named, so the log holds nothing the user typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    Key,
    Click,
}

/// A kind of miss that can repeat. A launch has one start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MissKind {
    ViewQuery,
    IssueFromDatabase,
    IssueFromGithub,
    Jump,
    Input,
    LongFrame,
}

/// A script that ran in a long frame, as the browser names it.
#[derive(Debug, Clone, PartialEq, Deserialize, Type)]
pub struct Script {
    /// The script's caller, such as `HTMLButtonElement.onclick` or `FrameRequestCallback`.
    pub invoker: String,
    /// Nothing, when the browser doesn't know it.
    pub function: Option<String>,
    /// The script's file, without the window's origin, or nothing when the browser doesn't know it.
    pub source: Option<String>,
    pub ms: f64,
}

impl Miss {
    /// The line the window's miss writes to `tsugiki.log`.
    pub fn line(&self) -> String {
        match self {
            Self::Startup { ms } => format!("Start-up to a usable Workbench took {ms:.0} ms"),
            Self::ViewQuery { ms } => format!("Slow view query: {ms:.1} ms"),
            Self::IssueFromDatabase { number, ms } => {
                format!("Slow opening of #{number} from the database: {ms:.1} ms")
            }
            Self::IssueFromGithub { number, ms } => {
                format!("Slow opening of #{number} from GitHub: {ms:.0} ms")
            }
            Self::Jump { ms } => format!("Slow jump in a diff: {ms:.1} ms to its rows"),
            Self::Input { input, ms } => {
                let input = match input {
                    Input::Key => "key",
                    Input::Click => "click",
                };
                format!("Slow {input}: {ms:.1} ms to the next paint")
            }
            Self::LongFrame { ms, scripts } => {
                let scripts = if scripts.is_empty() {
                    "no script known".to_owned()
                } else {
                    scripts
                        .iter()
                        .map(Script::words)
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                format!("Long frame: {ms:.1} ms, {scripts}")
            }
            Self::Capped { of, limit } => {
                let misses = match of {
                    MissKind::ViewQuery => "slow view queries",
                    MissKind::IssueFromDatabase => "slow openings from the database",
                    MissKind::IssueFromGithub => "slow openings from GitHub",
                    MissKind::Jump => "slow jumps in a diff",
                    MissKind::Input => "slow keys and clicks",
                    MissKind::LongFrame => "long frames",
                };
                format!("{limit} {misses} in a minute, so no more are logged until it ends")
            }
        }
    }
}

impl Script {
    fn words(&self) -> String {
        let function = self
            .function
            .as_ref()
            .map_or_else(String::new, |function| format!(" in {function}"));
        let source = self
            .source
            .as_ref()
            .map_or_else(String::new, |source| format!(" at {source}"));
        format!("{:.1} ms from {}{function}{source}", self.ms, self.invoker)
    }
}

#[cfg(test)]
mod tests {
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
}
