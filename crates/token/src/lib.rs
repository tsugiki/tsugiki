//! Where Tsugiki's GitHub token comes from, the one place it goes, and how a secret is hidden (ADR 0019).
//!
//! Nothing outside this crate can read a [`Token`]'s string. It leaves only in the `Authorization` header
//! of a request that [`Https`] sends to `api.github.com`. The README says this for someone who doesn't read
//! Rust.

#![forbid(unsafe_code)]

mod live;

use std::fmt;
use std::io;

use rustls::CertificateError;
use url::Url;
use url::form_urlencoded;

pub use live::{Https, run_gh};

/// The one host [`Https`] sends requests to (ADR 0005).
const API_HOST: &str = "api.github.com";
/// Where GraphQL requests go.
const GRAPHQL: &str = "https://api.github.com/graphql";
/// Where a [`RestPath`] starts.
const REST_ROOT: &str = "https://api.github.com";
/// How GitHub's tokens start. [`Token::hide_in`] covers the token in use whatever its shape.
const TOKEN_PREFIXES: [&str; 6] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];
/// What `RestPath::parse` refuses: each could make the rest of an address name another host, or part of
/// one.
const NOT_IN_A_PATH: [char; 4] = ['@', '?', '#', '\\'];

/// What running `gh` printed.
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
}

/// Runs `gh` with the given arguments: [`run_gh`] in the app, a stand-in in tests.
pub type RunGh = fn(&[&str]) -> io::Result<GhOutput>;

/// A GitHub token, held in memory. Nothing outside this crate can read its string. It has no `Serialize`,
/// so no command or event can carry it, and printing it shows `Token(***)`. A copy goes to the logger, which
/// hides it in each line.
#[derive(Clone)]
pub struct Token(String);

/// Why `gh` gave no token.
#[derive(Debug)]
pub enum NoToken {
    /// `gh` couldn't be run: it isn't installed where Tsugiki looks, or it didn't answer in time.
    GhMissing(io::Error),
    /// `gh` ran and failed, as it does when nobody has signed in.
    GhFailed,
    /// `gh` succeeded and printed no token.
    GhPrintedNothing,
}

impl Token {
    /// Asks `gh` for its token for github.com.
    pub fn from_gh(run: RunGh) -> Result<Self, NoToken> {
        let output =
            run(&["auth", "token", "--hostname", "github.com"]).map_err(NoToken::GhMissing)?;
        let token = output.stdout.trim();
        if !output.success {
            Err(NoToken::GhFailed)
        } else if token.is_empty() {
            Err(NoToken::GhPrintedNothing)
        } else {
            Ok(Self(token.to_owned()))
        }
    }

    /// `text` with each copy of this token replaced by `replacement`. [`hide_token_shapes`] covers the
    /// tokens it doesn't know.
    pub fn hide_in(&self, text: &str, replacement: &str) -> String {
        text.replace(self.0.as_str(), replacement)
    }

    /// The value of the `Authorization` header, the one place the token goes.
    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

/// `text` with each run of token characters after a prefix GitHub gives its tokens, such as `ghp_`,
/// replaced by `replacement`. The prefix can follow anything, an escape such as `\n` in raw JSON included.
/// A prefix with nothing after it is only words.
pub fn hide_token_shapes(text: &str, replacement: &str) -> String {
    TOKEN_PREFIXES.iter().fold(text.to_owned(), |text, prefix| {
        hide_after_prefix(&text, prefix, replacement)
    })
}

fn hide_after_prefix(text: &str, prefix: &str, replacement: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(prefix) {
        let tail = after.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        kept.push_str(before);
        kept.push_str(if tail.len() == after.len() {
            prefix
        } else {
            replacement
        });
        rest = tail;
    }
    kept.push_str(rest);
    kept
}

/// A path on GitHub's REST API, from its first `/`, with an optional query. No `RestPath` can name a host
/// other than `api.github.com`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestPath {
    path: String,
    query: Option<String>,
}

impl RestPath {
    /// The path `text` spells, if it starts with `/` and holds none of `@`, `?`, `#` and `\`, nor a `.` or
    /// `..` segment, which would send the request to another path than the one it names. The query comes
    /// through [`RestPath::with_query`], which encodes it.
    pub fn parse(text: &str) -> Option<Self> {
        let is_path = text.starts_with('/')
            && !text.contains(NOT_IN_A_PATH)
            && !text.split('/').any(is_dot_segment);
        is_path.then(|| Self {
            path: text.to_owned(),
            query: None,
        })
    }

    /// The same path with `pairs` as its query, each name and value encoded.
    #[must_use]
    pub fn with_query(self, pairs: &[(&str, &str)]) -> Self {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs)
            .finish();
        Self {
            query: Some(query),
            ..self
        }
    }

    /// The address this path names, if it is on `api.github.com` over HTTPS. Parsing keeps it there, and
    /// this checks again before the token goes out.
    fn url(&self) -> Option<Url> {
        let mut url = Url::parse(REST_ROOT).ok()?;
        url.set_path(&self.path);
        url.set_query(self.query.as_deref());
        is_github(&url).then_some(url)
    }
}

impl fmt::Display for RestPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)?;
        match &self.query {
            Some(query) => write!(f, "?{query}"),
            None => Ok(()),
        }
    }
}

/// Whether `segment` is `.` or `..`, which an address resolves away, written plainly or as `%2e`.
fn is_dot_segment(segment: &str) -> bool {
    let plain = segment.to_ascii_lowercase().replace("%2e", ".");
    plain == "." || plain == ".."
}

/// Whether `url` reaches `api.github.com` over HTTPS on its usual port, with no name or password in it.
fn is_github(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(API_HOST)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

/// What GitHub answered, as text. The core parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// The body of a GraphQL answer. A REST answer leaves it empty, since only its headers are read.
    pub body: String,
    /// The `Date` header.
    pub date: Option<String>,
    /// The `x-ratelimit-reset` header.
    pub ratelimit_reset: Option<String>,
    /// The `ETag` header.
    pub etag: Option<String>,
}

/// GitHub couldn't be reached at all. Each kind carries the whole cause, for the log: a proxy that
/// inspects TLS, for example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreachable {
    /// No network, no answer in time, an address that isn't GitHub's, or a connection that failed some other
    /// way, a certificate that failed for a reason other than its dates among them.
    Offline(String),
    /// GitHub's certificate isn't valid at this computer's date and time. The clock may be wrong, or the
    /// certificate may have expired, and the check can't tell which.
    CertificateDates(String),
}

/// Sorts a failed request by what the user can do about it.
fn unreachable(cause: &ureq::Error) -> Unreachable {
    let text = cause.to_string();
    if is_outside_certificate_dates(cause) {
        Unreachable::CertificateDates(text)
    } else {
        Unreachable::Offline(text)
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

#[cfg(test)]
mod tests;
