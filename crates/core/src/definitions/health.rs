//! What Settings › Advanced says of each language server (#683): where it is at, the version Tsugiki found
//! and the versions we tested, and the problem it last had, which the Definition peek says too.
//!
//! Tsugiki checks a server when the user sets its path: it runs the program once with `--version`, which
//! has to name the server, and clangd's built-in headers have to be beside it. A start runs no check, and
//! the server names its version answering `initialize`. While clangd runs, what it reports may give it a
//! problem as well (#684).

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
/// in words of the window's. The servers' thread owns it. The first three keep the server from answering.
/// With the last three, clangd runs and answers, but may find nothing, or a declaration in place of a
/// definition (#684).
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
    /// clangd's own headers, such as `stddef.h`, aren't in `lib\clang\<N>\include` beside its folder, as
    /// when only `clangd.exe` was unpacked from its archive.
    BuiltInHeadersMissing,
    /// clangd can't find `header`, which a file includes in angle brackets and the Repository doesn't hold:
    /// a header of the system's, such as `stdlib.h`, which on Windows comes with the MSVC build tools.
    SystemHeadersMissing { header: String },
    /// clangd hasn't reported its index within a minute of the first file opened, so it finds a function
    /// defined in another file at its declaration in a header.
    NotIndexing,
}

impl ServerProblem {
    /// The problem of a server that exited on its own as `exit` says.
    pub(super) fn stopped(exit: Exit) -> Self {
        Self::Stopped {
            code: exit.code,
            stderr_tail: exit.stderr_tail,
        }
    }

    /// How much the problem of a server that runs explains of what it fails to find, from 1 up, or 0 for a
    /// problem that keeps it from answering. Missing built-in headers make the system's go missing too, and
    /// either leaves the index little to hold.
    pub(super) fn weight(&self) -> u8 {
        match self {
            Self::NotThisServer { .. } | Self::DidNotStart { .. } | Self::Stopped { .. } => 0,
            Self::NotIndexing => 1,
            Self::SystemHeadersMissing { .. } => 2,
            Self::BuiltInHeadersMissing => 3,
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

/// What the servers' thread knows of a server's program, whichever Repository runs it: its version, and
/// what its `--version` check said.
#[derive(Debug, Clone, Default)]
pub(super) struct Health {
    pub(super) version: Option<String>,
    pub(super) check: Check,
    /// What a look at the program's folder found it lacks, which each start looks for again: clangd's
    /// built-in headers (#684).
    pub(super) flaw: Option<ServerProblem>,
}

/// Where a server's `--version` check stands.
#[derive(Debug, Clone, Default)]
pub(super) enum Check {
    /// Nothing against it: its path hasn't changed since Tsugiki started, the check passed, or a server
    /// started from the program since.
    #[default]
    Clear,
    /// Its path changed, and `--version` hasn't answered yet.
    Waiting,
    /// `--version` had this problem, and no server has started from the program since.
    Failed(ServerProblem),
}

/// What last happened to a Repository's server that its status says.
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
    /// A program whose path just changed, which `--version` checks now.
    pub(super) fn checking() -> Self {
        Self {
            version: None,
            check: Check::Waiting,
            flaw: None,
        }
    }

    /// Notes that a server started from the program, which clears what its check found, with the version it
    /// named answering `initialize`, if it did, and `flaw`, what a look at its folder found it lacks now.
    pub(super) fn started(&mut self, version: Option<String>, flaw: Option<ServerProblem>) {
        self.check = Check::Clear;
        self.flaw = flaw;
        if version.is_some() {
            self.version = version;
        }
    }

    /// Notes what checking the program found, unless a server has started from it since: what a running
    /// server says outranks a check that answered later.
    pub(super) fn checked(&mut self, outcome: Result<Found, ServerProblem>) {
        if !matches!(self.check, Check::Waiting) {
            return;
        }
        match outcome {
            Ok(found) => {
                self.check = Check::Clear;
                self.version = found.version;
                self.flaw = found.flaw;
            }
            Err(problem) => self.check = Check::Failed(problem),
        }
    }

    /// The problem of a server of this program that runs, to which `last` happened, which may explain why
    /// it found nothing, if it has one: what the program lacks, or what the server reported.
    pub(super) fn misleads<'a>(&'a self, last: &'a Last) -> Option<&'a ServerProblem> {
        self.flaw.as_ref().or_else(|| last.misleads())
    }

    /// The row of server `server`, which is `presence`, and to which `last` happened, in the Repository the
    /// rows show.
    pub(super) fn row(&self, server: ServerName, presence: Presence, last: &Last) -> ServerRow {
        let status = match (presence, &self.check, &self.flaw, last) {
            (Presence::NotSetUp, ..) => ServerStatus::NotSetUp,
            (_, Check::Failed(problem), ..)
            | (_, _, Some(problem), _)
            | (.., Last::Problem(problem)) => ServerStatus::Problem {
                problem: problem.clone(),
            },
            (Presence::Running, ..) => ServerStatus::Running,
            (Presence::Stopped, .., Last::Idle) => ServerStatus::Idle,
            (Presence::Stopped, .., Last::Nothing) => ServerStatus::Ready,
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

impl Last {
    /// Notes that the server reports its index, so it no longer has the problem of a server that doesn't,
    /// however late the report came.
    pub(super) fn indexed(&mut self) {
        if let Self::Problem(ServerProblem::NotIndexing) = self {
            *self = Self::Nothing;
        }
    }

    /// Notes `problem` of a server that runs, unless it has one that explains as much or more, and says
    /// whether it did. A problem that kept it from answering is behind it, as it runs.
    pub(super) fn troubled(&mut self, problem: ServerProblem) -> bool {
        let weight = match self {
            Self::Problem(problem) => problem.weight(),
            Self::Nothing | Self::Idle => 0,
        };
        let worse = problem.weight() > weight;
        if worse {
            *self = Self::Problem(problem);
        }
        worse
    }

    /// The problem a server that runs reported, which may explain why it found nothing, if it has one.
    fn misleads(&self) -> Option<&ServerProblem> {
        match self {
            Self::Problem(problem) if problem.weight() > 0 => Some(problem),
            Self::Problem(_) | Self::Nothing | Self::Idle => None,
        }
    }
}

/// What checking found of a program whose `--version` named the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    /// The version `--version` printed, if it printed one.
    pub(super) version: Option<String>,
    /// What a look at its folder found it lacks, if anything.
    pub(super) flaw: Option<ServerProblem>,
}

/// What checking the program at `program` for server `name` finds, or what is wrong: its `--version` has to
/// name the server, and clangd's built-in headers have to be beside it (#684). It waits for the program, so
/// it runs off the servers' thread.
pub(super) fn check(name: ServerName, program: &Path) -> Result<Found, ServerProblem> {
    let version = read_check(name, tsugiki_lsp::version_of(program, CHECK_PATIENCE))?;
    Ok(Found {
        version,
        flaw: flaw(name, program),
    })
}

/// What a look at the folder of the program of server `name` at `program` finds it lacks: clangd's
/// built-in headers.
pub(super) fn flaw(name: ServerName, program: &Path) -> Option<ServerProblem> {
    (name == ServerName::Clangd && !super::clangd::has_built_ins(program))
        .then_some(ServerProblem::BuiltInHeadersMissing)
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
