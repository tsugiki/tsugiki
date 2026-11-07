//! What touches the outside world: the real network, the real `gh` and the real `git`. Tests replace the
//! network and `gh` (ADR 0009), so this file is left out of the coverage gate, and the release checks
//! exercise it against GitHub. Tests run the real `git` through it.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file clippy.toml lets use the real network and real programs"
)]

use std::ffi::OsStr;
use std::io;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::{Duration, Instant};

use rustls::CertificateError;
use ureq::Agent;

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

/// Starts `git` with `args`, its error output piped for its progress, which it writes in English.
///
/// A Repository git can't read fails rather than asking for a password nobody sees: no terminal prompt, and
/// no credential helper, since Git Credential Manager on Windows would open a sign-in window.
pub(crate) fn start_git(args: &[&OsStr]) -> io::Result<(Child, ChildStderr)> {
    let mut command = Command::new("git");
    let _ = command
        .args(["-c", "credential.helper="])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    without_window(&mut command);
    // Its own process group, so `stop_git` can end the helpers it starts along with it.
    #[cfg(unix)]
    let _ = std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut git = command.spawn()?;
    match git.stderr.take() {
        Some(progress) => Ok((git, progress)),
        None => {
            stop_git(&mut git);
            Err(io::Error::other("git's error output wasn't piped"))
        }
    }
}

/// Ends `git` and waits for it. git hands a download to helpers (`git-remote-https`, `index-pack`), which
/// would otherwise go on downloading after Tsugiki quits, so this ends them too. A failure leaves a trace.
pub(crate) fn stop_git(git: &mut Child) {
    if let Err(cause) = end_git_and_helpers(git) {
        log::warn!("Tsugiki can't stop git: {cause}");
    }
}

fn end_git_and_helpers(git: &mut Child) -> io::Result<()> {
    let id = git.id().to_string();
    #[cfg(windows)]
    let mut tree = {
        let mut taskkill = Command::new("taskkill");
        let _ = taskkill.args(["/F", "/T", "/PID", &id]);
        taskkill
    };
    #[cfg(not(windows))]
    let mut tree = {
        let mut kill = Command::new("kill");
        let _ = kill.args(["-s", "KILL", "--", &format!("-{id}")]);
        kill
    };
    let _ = tree
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    without_window(&mut tree);
    let ended = tree.status()?;
    if !ended.success() {
        // git ended between the two calls, or the tree couldn't be ended, so end git at least.
        git.kill()?;
    }
    git.wait().map(drop)
}

/// Keeps a console program from opening a window on Windows, where Tsugiki itself has no console.
fn without_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = command.creation_flags(CREATE_NO_WINDOW);
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
mod tests {
    use rustls::pki_types::UnixTime;

    use super::*;

    /// A handshake that rustls failed, as ureq hands it back.
    fn handshake_failed(error: CertificateError) -> ureq::Error {
        let tls = rustls::Error::InvalidCertificate(error);
        ureq::Error::Io(io::Error::new(io::ErrorKind::InvalidData, tls))
    }

    #[test]
    fn a_certificate_outside_its_dates_is_about_the_clock() {
        // webpki's verifier, which ureq uses, gives the two with context.
        let time = UnixTime::since_unix_epoch(Duration::from_secs(1_800_000_000));
        let causes = [
            handshake_failed(CertificateError::ExpiredContext {
                time,
                not_after: UnixTime::since_unix_epoch(Duration::from_secs(1_700_000_000)),
            }),
            handshake_failed(CertificateError::NotValidYetContext {
                time,
                not_before: UnixTime::since_unix_epoch(Duration::from_secs(1_900_000_000)),
            }),
            handshake_failed(CertificateError::Expired),
            handshake_failed(CertificateError::NotValidYet),
        ];
        for cause in causes {
            assert_eq!(unreachable(cause), Unreachable::CertificateDates);
        }
    }

    #[test]
    fn any_other_failure_is_offline() {
        let causes = [
            handshake_failed(CertificateError::UnknownIssuer),
            handshake_failed(CertificateError::ExpiredRevocationList),
            ureq::Error::Rustls(rustls::Error::InvalidCertificate(CertificateError::Expired)),
            ureq::Error::Io(io::Error::new(io::ErrorKind::TimedOut, "timed out")),
            ureq::Error::HostNotFound,
        ];
        for cause in causes {
            assert_eq!(unreachable(cause), Unreachable::Offline);
        }
    }
}
