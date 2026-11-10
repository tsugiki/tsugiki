//! The only code in Tsugiki that reaches the network or runs `gh`. Tests replace both (ADR 0009), so this
//! file is left out of the coverage gate, and the release checks exercise it against GitHub.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file clippy.toml lets reach the network and run gh"
)]

use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use ureq::http::Response;
use ureq::typestate::WithoutBody;
use ureq::{Agent, Body, RequestBuilder};

use crate::{API_HOST, GRAPHQL, GhOutput, Reply, RestPath, Token, Unreachable, unreachable};

/// How long one request gets, from connecting to its last byte. GitHub ends a GraphQL query itself after
/// 10 s, so this leaves 5 s for the network, and it strikes when the network or a proxy has stopped
/// answering. Nobody has measured the 5 s. On a link too slow for it Tsugiki says Offline, and the next
/// refresh tries again. `npm run smoke` needs a request to wait at its proxy until the page is ready, so a
/// much shorter limit would fail its first check.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long `gh` gets. It's longer than a request's limit because without a token the session ends, and
/// nothing asks `gh` again until Tsugiki restarts.
const GH_TIMEOUT: Duration = Duration::from_secs(30);
/// Keeps a console program from opening a window on Windows, where Tsugiki itself has no console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The only HTTP client in Tsugiki. It reaches `api.github.com` over HTTPS and follows no redirects, so a
/// request can't end up on another host.
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

    /// Sends `body`, a GraphQL request as JSON, with `token`.
    pub fn post_graphql(&self, token: &Token, body: String) -> Result<Reply, Unreachable> {
        let response = self
            .agent
            .post(GRAPHQL)
            .header("Authorization", token.bearer())
            .content_type("application/json")
            .send(body)
            .map_err(|cause| unreachable(&cause))?;
        with_body(response)
    }

    /// Asks for `path` with `token`. With `etag`, GitHub answers 304 if nothing changed since that answer.
    /// Only the status and headers come back: a refresh reads nothing else, so the body isn't downloaded.
    pub fn get_rest(
        &self,
        token: &Token,
        path: &RestPath,
        etag: Option<&str>,
    ) -> Result<Reply, Unreachable> {
        let mut asking = self
            .public_request(path)?
            .header("Authorization", token.bearer());
        if let Some(etag) = etag {
            asking = asking.header("If-None-Match", etag);
        }
        let response = asking.call().map_err(|cause| unreachable(&cause))?;
        Ok(headers(&response))
    }

    /// Asks for `path` with no token, for what GitHub shows anyone, such as Tsugiki's latest release. It
    /// takes no [`Token`], so it has no way to send one. The body comes back with the status and headers.
    pub fn get_public(&self, path: &RestPath) -> Result<Reply, Unreachable> {
        let response = self
            .public_request(path)?
            .call()
            .map_err(|cause| unreachable(&cause))?;
        with_body(response)
    }

    /// A GET of `path` on `api.github.com`, with no token yet. `get_rest` adds one, and `get_public` sends it
    /// as it is.
    fn public_request(&self, path: &RestPath) -> Result<RequestBuilder<WithoutBody>, Unreachable> {
        let url = path
            .url()
            .ok_or_else(|| Unreachable::Offline(format!("{path} isn't on {API_HOST}")))?;
        Ok(self
            .agent
            .get(url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"))
    }
}

impl Default for Https {
    fn default() -> Self {
        Self::new()
    }
}

/// The status, the headers the core reads and the body of `response`.
fn with_body(mut response: Response<Body>) -> Result<Reply, Unreachable> {
    let reply = headers(&response);
    Ok(Reply {
        body: response
            .body_mut()
            .read_to_string()
            .map_err(|cause| unreachable(&cause))?,
        ..reply
    })
}

/// The status and the headers the core reads of `response`, with an empty body.
fn headers(response: &Response<Body>) -> Reply {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    Reply {
        status: response.status().as_u16(),
        body: String::new(),
        date: header("date"),
        ratelimit_reset: header("x-ratelimit-reset"),
        etag: header("etag"),
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

/// Runs one program, with no console window on Windows, and gives up on it after `GH_TIMEOUT`.
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
        let _ = command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + GH_TIMEOUT;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            child.kill()?;
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    Ok(GhOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

#[cfg(test)]
mod tests;
