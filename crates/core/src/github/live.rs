//! What touches the outside world: the real network and the real `gh`. Tests replace both (ADR 0009),
//! so this file is left out of the coverage gate, and the release checks exercise it against GitHub.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file clippy.toml lets use the real network and real programs"
)]

use std::io;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ureq::Agent;

use super::{Reply, Request, Transport, Unreachable};
use crate::token::{GhOutput, Token};

/// The only address the core sends requests to (ADR 0005).
const ENDPOINT: &str = "https://api.github.com/graphql";
const TIMEOUT: Duration = Duration::from_secs(30);

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
            .timeout_global(Some(TIMEOUT))
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
        let body = serde_json::json!({ "query": request.query }).to_string();
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
        Ok(Reply {
            status: response.status().as_u16(),
            reset,
            body: response.body_mut().read_to_string().map_err(unreachable)?,
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

/// The status bar only says GitHub can't be reached, so this is the one place the reason shows: a proxy that
/// inspects TLS, for example.
fn unreachable(cause: ureq::Error) -> Unreachable {
    log::warn!("A request to GitHub failed: {cause}");
    Unreachable
}

/// Runs one program, with no console window on Windows, and gives up on it after `TIMEOUT`.
fn run_program(program: &str, args: &[&str]) -> io::Result<GhOutput> {
    let mut command = Command::new(program);
    let _ = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + TIMEOUT;
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
