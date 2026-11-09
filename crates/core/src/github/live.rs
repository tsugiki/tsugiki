//! What touches the outside world: the real network, the real `gh` and the real `git`. Tests replace the
//! network and `gh` (ADR 0009), so this file is left out of the coverage gate, and the release checks
//! exercise it against GitHub. Tests run the real `git` through it.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file clippy.toml lets use the real network and real programs"
)]

use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::{ChildStderr, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use process_wrap::std::{ChildWrapper, CommandWrap};
use rustls::CertificateError;
use ureq::Agent;
#[cfg(windows)]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::github::clock::http_date;
use crate::token::{GhOutput, Token};

/// The only address the core sends requests to (ADR 0005).
const ENDPOINT: &str = "https://api.github.com/graphql";
const REST_ROOT: &str = "https://api.github.com";
/// How long one request gets, from connecting to its last byte. GitHub ends a GraphQL query itself after
/// 10 s, so this leaves 5 s for the network, and it strikes when the network or a proxy has stopped
/// answering. Nobody has measured the 5 s. On a link too slow for it Tsugiki says Offline, and the next
/// refresh tries again. `npm run smoke` needs a request to wait at its proxy until the page is ready, so a
/// much shorter limit would fail its first check.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long `gh` gets. It's longer than a request's limit because without a token the session ends, and
/// nothing asks `gh` again until Tsugiki restarts.
const GH_TIMEOUT: Duration = Duration::from_secs(30);

/// GitHub over HTTPS. It follows no redirects, so a request can't end up on another host.
pub struct Https {
    agent: Agent,
}

impl Https {
    pub fn new() -> Self {
        let config = Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent(concat!("tsugiki/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl Default for Https {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for Https {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let body = serde_json::json!({ "query": request.query, "variables": request.variables })
            .to_string();
        let mut response = self
            .agent
            .post(ENDPOINT)
            .header("Authorization", format!("Bearer {}", token.expose()))
            .content_type("application/json")
            .send(body)
            .map_err(unreachable)?;
        let reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|value| value.to_str().ok()?.parse().ok());
        let date = response
            .headers()
            .get("date")
            .and_then(|value| http_date(value.to_str().ok()?));
        Ok(Reply {
            status: response.status().as_u16(),
            reset,
            date,
            body: response.body_mut().read_to_string().map_err(unreachable)?,
        })
    }

    fn get(
        &self,
        token: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        let mut asking = self
            .agent
            .get(format!("{REST_ROOT}{}", request.path))
            .header("Authorization", format!("Bearer {}", token.expose()))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(etag) = request.etag {
            asking = asking.header("If-None-Match", etag);
        }
        let response = asking.call().map_err(unreachable)?;
        let headers = response.headers();
        Ok(ConditionalReply {
            status: response.status().as_u16(),
            etag: headers
                .get("etag")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            date: headers
                .get("date")
                .and_then(|value| http_date(value.to_str().ok()?)),
        })
    }
}

/// Where to look for `gh`. An app opened from Finder doesn't get the shell's PATH, so on macOS this also
/// names Homebrew's folders.
#[cfg(target_os = "macos")]
const GH_PROGRAMS: [&str; 3] = ["gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh"];
#[cfg(not(target_os = "macos"))]
const GH_PROGRAMS: [&str; 1] = ["gh"];

/// Runs `gh`.
pub fn run_gh(args: &[&str]) -> io::Result<GhOutput> {
    let mut outcome = Err(io::Error::from(io::ErrorKind::NotFound));
    for program in GH_PROGRAMS {
        outcome = run_program(program, args);
        if !matches!(&outcome, Err(error) if error.kind() == io::ErrorKind::NotFound) {
            break;
        }
    }
    outcome
}

/// The status bar says only that GitHub can't be reached, or that its certificate's dates don't fit the
/// clock, so this is the one place the whole reason shows: a proxy that inspects TLS, for example.
fn unreachable(cause: ureq::Error) -> Unreachable {
    log::warn!("A request to GitHub failed: {cause}");
    if is_outside_certificate_dates(&cause) {
        Unreachable::CertificateDates
    } else {
        Unreachable::Offline
    }
}

/// True when `cause` is a certificate that has expired or isn't valid yet. A failed handshake reaches ureq
/// as an I/O error that wraps rustls's own: ureq's `Error::Rustls` comes only from setting up a connection,
/// before any certificate arrives.
fn is_outside_certificate_dates(cause: &ureq::Error) -> bool {
    let tls = if let ureq::Error::Io(io) = cause {
        io.get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    } else {
        None
    };
    matches!(
        tls,
        Some(rustls::Error::InvalidCertificate(
            CertificateError::Expired
                | CertificateError::ExpiredContext { .. }
                | CertificateError::NotValidYet
                | CertificateError::NotValidYetContext { .. }
        ))
    )
}

/// Starts `git` with `args` and `input` on its standard input, its error output piped for its progress, which
/// it writes in English.
///
/// A Repository git can't read fails rather than asking for a password nobody sees: no terminal prompt, and
/// no credential helper, since Git Credential Manager on Windows would open a sign-in window.
pub(crate) fn start_git(args: &[&OsStr], input: &[u8]) -> io::Result<(StartedGit, ChildStderr)> {
    let mut command = Command::new("git");
    let _ = command
        .args(["-c", "credential.helper="])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // git hands a download to helpers (`git-remote-https`, `index-pack`), and the `git` on PATH on Windows
    // is a launcher that starts the real one. Each helper joins git's job object, or its process group, as
    // it starts, so `StartedGit::stop` ends them all. Reading the tree when stopping misses a helper that
    // starts while it reads, and that helper goes on holding files in the mirror (#208).
    let mut command = CommandWrap::from(command);
    #[cfg(windows)]
    let _ = command
        .wrap(process_wrap::std::CreationFlags(CREATE_NO_WINDOW))
        .wrap(process_wrap::std::JobObject);
    #[cfg(not(windows))]
    let _ = command.wrap(process_wrap::std::ProcessGroup::leader());
    let mut git = StartedGit(command.spawn()?);
    // On a thread of its own, so a git that writes before it reads can't leave both waiting on a full pipe.
    // A git that stops reading says why on its error output. Closing the input tells git that is all.
    if let Some(mut stdin) = git.0.stdin().take() {
        let input = input.to_vec();
        let _ = thread::spawn(move || drop(stdin.write_all(&input)));
    }
    match git.0.stderr().take() {
        Some(progress) => Ok((git, progress)),
        None => {
            git.stop();
            Err(io::Error::other("git's error output wasn't piped"))
        }
    }
}

/// A git that `start_git` started, with the helpers it starts in turn.
pub(crate) struct StartedGit(Box<dyn ChildWrapper>);

impl StartedGit {
    /// Waits for git to end, and says if it did what it was asked.
    pub(crate) fn succeeded(&mut self) -> bool {
        self.0.wait().is_ok_and(|status| status.success())
    }

    /// Ends git and its helpers, so none goes on downloading after Tsugiki quits, and waits for git. A helper
    /// can take a moment longer to end, and git's error output closes once the last one has. A failure
    /// leaves a trace.
    pub(crate) fn stop(&mut self) {
        if let Err(cause) = self.0.start_kill() {
            log::warn!("Tsugiki can't stop git's helpers: {cause}");
            // git at least, so the wait doesn't last until git ends on its own.
            if let Err(cause) = self.0.inner_mut().start_kill() {
                log::warn!("Tsugiki can't stop git: {cause}");
            }
        }
        if let Err(cause) = self.0.wait() {
            log::warn!("Tsugiki can't wait for git: {cause}");
        }
    }
}

/// Runs a `git` that only reads local files, with `args`, and gives its output. Its last line of error output
/// says why it failed.
pub(crate) fn read_git(args: &[&OsStr]) -> io::Result<String> {
    let mut command = local_git(args);
    let _ = command.stdin(Stdio::null());
    let output = command.output()?;
    git_output(&output).map(|stdout| String::from_utf8_lossy(stdout).into_owned())
}

/// Runs a `git` that only reads local files, with `args` and `input` on its standard input, and gives its
/// output as git wrote it. Its last line of error output says why it failed.
pub(crate) fn read_git_bytes(args: &[&OsStr], input: Vec<u8>) -> io::Result<Vec<u8>> {
    git_bytes(local_git(args), input)
}

/// The same, with the environment variables `envs` set for git.
pub(crate) fn read_git_bytes_with_env(
    args: &[&OsStr],
    input: Vec<u8>,
    envs: &[(&str, &OsStr)],
) -> io::Result<Vec<u8>> {
    let mut command = local_git(args);
    let _ = command.envs(envs.iter().copied());
    git_bytes(command, input)
}

fn git_bytes(mut command: Command, input: Vec<u8>) -> io::Result<Vec<u8>> {
    let _ = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut git = command.spawn()?;
    // On a thread of its own, so a git that writes before it has read everything can't leave both waiting on
    // a full pipe. Closing the input tells git that is all.
    if let Some(mut stdin) = git.stdin.take() {
        let _ = thread::spawn(move || drop(stdin.write_all(&input)));
    }
    let output = git.wait_with_output()?;
    git_output(&output).map(<[u8]>::to_vec)
}

/// A `git` with `args` that only reads local files, and writes in English with no console window.
fn local_git(args: &[&OsStr]) -> Command {
    let mut command = Command::new("git");
    let _ = command.args(args).env("LC_ALL", "C").env("LANGUAGE", "C");
    without_window(&mut command);
    command
}

/// The output of a `git`, or the gist of its error output if it failed.
fn git_output(output: &std::process::Output) -> io::Result<&[u8]> {
    if output.status.success() {
        Ok(&output.stdout)
    } else {
        let errors = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(gist(&errors).to_owned()))
    }
}

/// git's last `fatal:` or `error:` line, or else its last line. Hints follow the reason, as in
/// `fatal: detected dubious ownership`, whose last line is the command that adds a `safe.directory`.
fn gist(errors: &str) -> &str {
    errors
        .lines()
        .rev()
        .find(|line| line.starts_with("fatal: ") || line.starts_with("error: "))
        .or_else(|| errors.lines().last())
        .unwrap_or("")
}

/// Keeps a console program from opening a window on Windows, where Tsugiki itself has no console.
fn without_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = command.creation_flags(CREATE_NO_WINDOW.0);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Runs one program, with no console window on Windows, and gives up on it after `GH_TIMEOUT`.
fn run_program(program: &str, args: &[&str]) -> io::Result<GhOutput> {
    let mut command = Command::new(program);
    let _ = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    without_window(&mut command);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + GH_TIMEOUT;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            child.kill()?;
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    Ok(GhOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

#[cfg(test)]
mod tests;
