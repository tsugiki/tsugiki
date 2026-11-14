//! What Settings › Advanced says of each language server (#683): where it is at, the version Tsugiki found
//! and the versions we tested, and the problem it last had, which the Definition peek says too.
//!
//! Tsugiki checks a server when the user sets its path: it runs the program once with `--version`, which
//! has to name the server. A start runs no check, and the server names its version answering `initialize`.

use std::io;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tsugiki_lsp::Exit;

use super::ServerName;

#[cfg(test)]
mod tests;

/// The longest `--version` gets to answer before Tsugiki kills it, as a program that isn't a server may
/// never end.
const CHECK_PATIENCE: Duration = Duration::from_secs(10);

/// What is wrong with a language server, which its row in Settings › Advanced and the Definition peek say
/// in words of the window's. The servers' thread owns it, and more kinds can join it, such as what clangd
/// reports while it runs (#684).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerProblem {
    /// The program's `--version` doesn't name the server: the first line it printed, which may be empty.
    NotThisServer { first_line: String },
    /// The program didn't start, or didn't answer `initialize`: why, as the OS or the server said.
    DidNotStart { cause: String },
    /// The server exited on its own: its exit code, if it had one, and the end of its error output.
    Stopped {
        code: Option<i32>,
        stderr_tail: String,
    },
}

impl ServerProblem {
    /// The problem of a server that exited on its own as `exit` says.
    pub(super) fn stopped(exit: Exit) -> Self {
        Self::Stopped {
            code: exit.code,
            stderr_tail: exit.stderr_tail,
        }
    }
}

/// Where a language server is at, as its row in Settings › Advanced says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerStatus {
    /// `machine.toml` gives it no path.
    NotSetUp,
    /// It has a path, and runs for no Repository yet.
    Ready,
    /// It runs for a Repository.
    Running,
    /// It stopped as no lookup reached it for the idle time, and the next lookup starts it again.
    Idle,
    /// The last thing that happened to it went wrong. The next lookup tries it again.
    Problem { problem: ServerProblem },
}

/// A version of a server Tsugiki found, and whether it is one we tested. Another is used all the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FoundVersion {
    pub version: String,
    pub tested: bool,
}

/// One server's row in Settings › Advanced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ServerRow {
    pub server: ServerName,
    pub status: ServerStatus,
    /// The version `--version` printed when the user set its path, or the one the server named answering
    /// `initialize`, if either did.
    pub version: Option<FoundVersion>,
    /// The versions we tested, which the docs name too.
    pub tested: Vec<String>,
}

/// What the servers' thread knows of a server beyond whether it runs: its version, and what last happened
/// to it.
#[derive(Debug, Clone, Default)]
pub(super) struct Health {
    pub(super) version: Option<String>,
    pub(super) last: Last,
}

/// What last happened to a server that its status says.
#[derive(Debug, Clone, Default)]
pub(super) enum Last {
    /// Nothing to say: it started, or nothing has happened yet.
    #[default]
    Nothing,
    /// It stopped as no lookup reached it for the idle time.
    Idle,
    /// It had this problem.
    Problem(ServerProblem),
}

/// Whether a server has a path, and if so, whether it runs for a Repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    NotSetUp,
    Stopped,
    Running,
}

impl Health {
    /// Notes that the server started, which clears a problem it had, with the version it named answering
    /// `initialize`, if it did.
    pub(super) fn started(&mut self, version: Option<String>) {
        self.last = Last::Nothing;
        if version.is_some() {
            self.version = version;
        }
    }

    /// The row of server `server`, which is `presence`.
    pub(super) fn row(&self, server: ServerName, presence: Presence) -> ServerRow {
        let status = match (presence, &self.last) {
            (Presence::NotSetUp, _) => ServerStatus::NotSetUp,
            (_, Last::Problem(problem)) => ServerStatus::Problem {
                problem: problem.clone(),
            },
            (Presence::Running, _) => ServerStatus::Running,
            (Presence::Stopped, Last::Idle) => ServerStatus::Idle,
            (Presence::Stopped, Last::Nothing) => ServerStatus::Ready,
        };
        let tested = server.tested();
        let version = self.version.clone().map(|version| FoundVersion {
            tested: tested.contains(&version.as_str()),
            version,
        });
        ServerRow {
            server,
            status,
            version,
            tested: tested.iter().map(|version| (*version).to_owned()).collect(),
        }
    }
}

/// What `--version` of the program at `program` says of server `name`: the version, if it printed one, or
/// what is wrong. It waits for the program, so it runs off the servers' thread.
pub(super) fn check(name: ServerName, program: &Path) -> Result<Option<String>, ServerProblem> {
    read_check(name, tsugiki_lsp::version_of(program, CHECK_PATIENCE))
}

/// What `printed`, the text a program printed for `--version` or why it didn't run, says of server `name`.
/// The first line has to hold the server's name as a word, as `clangd version 23.1.0` does.
fn read_check(
    name: ServerName,
    printed: io::Result<String>,
) -> Result<Option<String>, ServerProblem> {
    let printed = printed.map_err(|cause| ServerProblem::DidNotStart {
        cause: cause.to_string(),
    })?;
    let first_line = (printed.lines().map(str::trim))
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let named =
        (first_line.split_whitespace()).any(|word| word.eq_ignore_ascii_case(name.as_str()));
    if named {
        Ok(version_in(first_line))
    } else {
        Err(ServerProblem::NotThisServer {
            first_line: first_line.to_owned(),
        })
    }
}

/// The version in `text`, a line a server printed or the version it named answering `initialize`: its first
/// word that starts with a digit, without what follows its last letter or digit. That is `0.0.103` in
/// `ty 0.0.103 (f9672a390 2026-11-13)`, and `23.1.0` in clangd's `clangd version 23.1.0`, which the address
/// of the commit it was built from follows.
pub(super) fn version_in(text: &str) -> Option<String> {
    let word =
        (text.split_whitespace()).find(|word| word.starts_with(|c: char| c.is_ascii_digit()))?;
    Some(
        word.trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_owned(),
    )
}
