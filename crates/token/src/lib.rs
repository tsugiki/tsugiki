//! Where Tsugiki's GitHub token comes from, the one place it goes, and how a secret is hidden (ADR 0019).
//!
//! Nothing outside this crate can read a [`Token`]'s string. It leaves only in the `Authorization` header
//! of a request that [`Https`] sends to `api.github.com`, and that request can only read, or make one of the
//! writes [`Write`] lists (ADR 0025). The README says this for someone who doesn't read Rust.

#![forbid(unsafe_code)]

mod live;

use std::fmt;
use std::io;

use rustls::CertificateError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use url::form_urlencoded;

pub use live::{GH_TIMEOUT, Https};

/// The variable `gh auth token` reads before its own sign-in, which Tsugiki reads first, and the only one it
/// reads.
const GH_TOKEN: &str = "GH_TOKEN";
/// The one host [`Https`] sends requests to (ADR 0005).
const API_HOST: &str = "api.github.com";
/// Where GraphQL requests go.
const GRAPHQL: &str = "https://api.github.com/graphql";
/// Where a [`RestPath`] starts.
const REST_ROOT: &str = "https://api.github.com";
/// How GitHub's tokens start. [`Token::hide_in`] covers the token in use whatever its shape.
const TOKEN_PREFIXES: [&str; 6] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];
/// The keyword each GraphQL operation that writes starts with. [`checked_read`] refuses a query that holds it
/// anywhere, and [`checked_selection`] a selection.
const MUTATION: &str = "mutation";
/// The characters [`checked_selection`] refuses besides `mutation`: `"` starts a string and `#` a comment,
/// inside which a brace wouldn't count.
const NOT_IN_A_SELECTION: [char; 2] = ['"', '#'];
/// What `RestPath::parse` refuses: each could make the rest of an address name another host, or part of
/// one.
const NOT_IN_A_PATH: [char; 4] = ['@', '?', '#', '\\'];

/// What running `gh` printed.
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
}

/// Runs `gh auth token --hostname github.com`, the one command Tsugiki runs `gh` for: `gh` itself in the
/// app, a stand-in in tests.
pub(crate) type GhAuthToken = fn() -> io::Result<GhOutput>;

/// The value of `GH_TOKEN`, if it is set and is Unicode: the process's own in the app, a stand-in in tests.
pub(crate) type ReadEnv = fn() -> Option<String>;

/// Where [`Token::find`] looks for a token. Its fields are private, so nothing outside this crate can run
/// the real `gh` through [`TokenFrom::LIVE`]. Tests build one through the `testing` module.
#[derive(Clone, Copy)]
pub struct TokenFrom {
    env: ReadEnv,
    gh: GhAuthToken,
}

impl TokenFrom {
    /// The process's environment and the real `gh`.
    pub const LIVE: Self = Self {
        env: live::read_env,
        gh: live::gh_auth_token,
    };
}

/// A GitHub token, held in memory. Nothing outside this crate can read its string. It has no `Serialize`,
/// so no command or event can carry it, and printing it shows `Token(***)`. A copy goes to the logger, which
/// hides it in each line, and the logger compares tokens to keep one copy of each.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

/// Why `gh` gave no token.
#[derive(Debug)]
pub enum NoToken {
    /// `gh` couldn't be run: it isn't installed where Tsugiki looks, or the system refused to run it.
    GhMissing(io::Error),
    /// `gh` didn't answer within [`GH_TIMEOUT`], so Tsugiki stopped it.
    GhTimedOut,
    /// `gh` ran and failed, as it does when nobody has signed in.
    GhFailed,
    /// `gh` succeeded and printed no token.
    GhPrintedNothing,
}

impl Token {
    /// The token in `GH_TOKEN` when it holds one, and otherwise gh's token for github.com. `gh auth token`
    /// gives `GH_TOKEN` first too, so the token is the same either way, but taking it here runs no `gh`.
    pub fn find(from: TokenFrom) -> Result<Self, NoToken> {
        match (from.env)() {
            Some(set) if !set.trim().is_empty() => Ok(Self(set.trim().to_owned())),
            _ => Self::from_gh(from.gh),
        }
    }

    /// Asks `gh` for its token for github.com.
    fn from_gh(run: GhAuthToken) -> Result<Self, NoToken> {
        let output = run().map_err(gh_unrun)?;
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

/// The [`NoToken`] for a `gh` that gave no answer: it timed out, or it couldn't be run.
fn gh_unrun(cause: io::Error) -> NoToken {
    if cause.kind() == io::ErrorKind::TimedOut {
        NoToken::GhTimedOut
    } else {
        NoToken::GhMissing(cause)
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

/// A GraphQL request as [`Https::post_graphql`] sends it. Serde refuses a body with any other key, or with
/// one of these twice, so the request GitHub gets is the one [`checked_read`] looked at.
///
/// The variables go out as `serde_json` writes them again, so a variable must come through a parse and a
/// write unchanged: text, a boolean, null, or a whole number that fits in 64 bits. A fraction can come back
/// with other digits, and a whole number past 64 bits as a fraction.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphQl {
    query: String,
    variables: Value,
}

/// The reason [`checked_read`] refused a GraphQL body.
#[derive(Debug, PartialEq, Eq)]
enum NotARead {
    /// The body isn't JSON of `query` and `variables` alone, each once. Serde's words say what it found.
    Unparsed(String),
    /// The query holds `mutation`, the keyword a write starts with.
    Mutation,
}

impl NotARead {
    fn unparsed(cause: serde_json::Error) -> Self {
        Self::Unparsed(cause.to_string())
    }
}

/// The [`Unreachable`] that [`Https::post_graphql`] answers for a body it won't send.
impl From<NotARead> for Unreachable {
    fn from(refused: NotARead) -> Self {
        Self::Refused(refused.to_string())
    }
}

impl fmt::Display for NotARead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparsed(cause) => write!(
                f,
                "the GraphQL body isn't a query and its variables: {cause}"
            ),
            Self::Mutation => write!(
                f,
                "the GraphQL query holds `{MUTATION}`, and only a write needs it"
            ),
        }
    }
}

/// `body`, a GraphQL request as JSON, written again from what was parsed, if it can only read. GraphQL
/// writes only through an operation that starts with `mutation`. Its keywords are ASCII and case-sensitive,
/// and it has no escape that spells one, so a query that holds the word nowhere writes nothing. The word
/// counts in a string, a comment or a longer word alike: an alias that holds it is refused, and a new name
/// fixes that.
fn checked_read(body: &str) -> Result<String, NotARead> {
    let request: GraphQl = serde_json::from_str(body).map_err(NotARead::unparsed)?;
    if request.query.contains(MUTATION) {
        return Err(NotARead::Mutation);
    }
    serde_json::to_string(&request).map_err(NotARead::unparsed)
}

/// GitHub's ID for a node, such as an Issue or a PR, which a read asks for it by and a write names it by.
/// Only tests can build one from text: elsewhere one comes from parsing JSON, such as GitHub's answer or what
/// the Store saved. It reaches a query or a mutation only as a variable, so no ID can change their text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

#[cfg(any(test, feature = "testing"))]
impl NodeId {
    /// The ID GitHub spells `id`.
    pub fn new(id: String) -> Self {
        Self(id)
    }
}

/// A write to GitHub that [`Https::post_write`] may send: one variant per mutation, each with typed inputs.
/// The crate holds each mutation's text, so nothing outside it can send a mutation that isn't listed here,
/// and the README has a row for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `addComment`: a comment on an Issue, or on a PR's Conversation, which `subject` names.
    AddComment { subject: NodeId, body: String },
}

/// The fixed text of a mutation, from which [`mutation_query`] writes its operation.
struct Mutation {
    /// The operation's name, which the core's log and recordings name the write by.
    operation: &'static str,
    /// The type of the mutation's one argument, `input`.
    input: &'static str,
    /// The field of GitHub's `Mutation` type it calls.
    field: &'static str,
}

const ADD_COMMENT: Mutation = Mutation {
    operation: "AddComment",
    input: "AddCommentInput",
    field: "addComment",
};

impl Write {
    /// The name of the write's operation, by which the core logs the write and finds its recording.
    pub fn operation(&self) -> &'static str {
        self.mutation().operation
    }

    /// The write's variables, its one `input` built from the typed inputs.
    pub fn variables(&self) -> Value {
        match self {
            Self::AddComment { subject, body } => {
                serde_json::json!({ "input": { "subjectId": subject, "body": body } })
            }
        }
    }

    /// The GraphQL document this write sends with `selection`, if [`checked_selection`] lets the selection
    /// through. Only [`Https::post_write`] sends it: [`Https::post_graphql`] refuses it, as it holds
    /// `mutation`.
    pub fn query(&self, selection: Selection<'_>) -> Result<String, BadSelection> {
        checked_selection(selection)?;
        Ok(mutation_query(self.mutation(), selection))
    }

    fn mutation(&self) -> &'static Mutation {
        match self {
            Self::AddComment { .. } => &ADD_COMMENT,
        }
    }
}

/// The part of a write the core supplies: the selection set of the mutation's payload, and the fragment
/// definitions it spreads. A write's answer then parses as a read's does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection<'a> {
    /// One selection set, from its `{` to the `}` that closes it: `{ commentEdge { node { ...C } } }`.
    pub payload: &'a str,
    /// The fragments `payload` spreads, such as `fragment C on IssueComment { id }`, or nothing.
    pub fragments: &'a str,
}

/// The reason [`checked_selection`] refused a write's selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadSelection {
    /// The selection holds `mutation`, the keyword a write starts with.
    Mutation,
    /// The selection holds `"` or `#`, which start a string or a comment, inside which a brace wouldn't
    /// count.
    StringOrComment,
    /// The payload isn't one selection set: it doesn't start with `{`, or the `}` that closes that `{`
    /// comes before its end, or is missing. Text after that `}` would sit beside the mutation's field.
    PayloadBraces,
    /// A `}` in the fragments closes a `{` they didn't open, or a `{` stays open.
    FragmentBraces,
}

impl fmt::Display for BadSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mutation => write!(f, "a write's selection holds `{MUTATION}`"),
            Self::StringOrComment => write!(f, "a write's selection holds `\"` or `#`"),
            Self::PayloadBraces => write!(f, "a write's payload isn't one selection set"),
            Self::FragmentBraces => write!(f, "the braces of a write's fragments don't pair up"),
        }
    }
}

/// The [`Unreachable`] that [`Https::post_write`] answers for a selection it won't send.
impl From<BadSelection> for Unreachable {
    fn from(refused: BadSelection) -> Self {
        Self::Refused(refused.to_string())
    }
}

/// Refuses a selection that could add to the write: one that holds `mutation`, `"` or `#`, a payload that
/// isn't one selection set, or fragments whose braces don't pair up. Without strings or comments each brace
/// counts, so a payload that closes its own `{` only at its end stays inside the mutation's one field, and
/// can't put a second field beside it. Fragments come after the operation has closed, where no field writes.
fn checked_selection(selection: Selection<'_>) -> Result<(), BadSelection> {
    let Selection { payload, fragments } = selection;
    if payload.contains(MUTATION) || fragments.contains(MUTATION) {
        return Err(BadSelection::Mutation);
    }
    if payload.contains(NOT_IN_A_SELECTION) || fragments.contains(NOT_IN_A_SELECTION) {
        return Err(BadSelection::StringOrComment);
    }
    if !is_one_selection_set(payload.trim()) {
        return Err(BadSelection::PayloadBraces);
    }
    let pair_up = brace_levels(fragments).all(|level| level >= 0)
        && brace_levels(fragments).last().unwrap_or(0) == 0;
    if !pair_up {
        return Err(BadSelection::FragmentBraces);
    }
    Ok(())
}

/// Whether `text` starts with `{` and the `}` that closes it is its last byte.
fn is_one_selection_set(text: &str) -> bool {
    let closed_at = brace_levels(text).position(|level| level <= 0);
    text.starts_with('{') && closed_at == text.len().checked_sub(1)
}

/// The level of `text`'s braces after each of its bytes, from 0 before the first: one more after each `{`,
/// one fewer after each `}`.
fn brace_levels(text: &str) -> impl Iterator<Item = isize> {
    text.bytes().scan(0, |level, byte| {
        match byte {
            b'{' => *level += 1,
            b'}' => *level -= 1,
            _ => {}
        }
        Some(*level)
    })
}

/// The operation of `mutation`, which asks the payload for `selection`.
fn mutation_query(mutation: &Mutation, selection: Selection<'_>) -> String {
    let Mutation {
        operation,
        input,
        field,
    } = mutation;
    let Selection { payload, fragments } = selection;
    let query = format!(
        "{MUTATION} {operation}($input: {input}!) {{ {field}(input: $input) {} }} {}",
        payload.trim(),
        fragments.trim()
    );
    query.trim_end().to_owned()
}

/// The JSON body of `write` with `selection`, as [`Https::post_write`] sends it.
fn write_body(write: &Write, selection: Selection<'_>) -> Result<String, BadSelection> {
    let query = write.query(selection)?;
    Ok(serde_json::json!({ "query": query, "variables": write.variables() }).to_string())
}

/// What GitHub answered, as text. The core parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// The body of a GraphQL answer or of [`Https::get_public`]'s. A signed-in REST answer leaves it empty,
    /// since only its headers are read.
    pub body: String,
    /// The `Date` header.
    pub date: Option<String>,
    /// The `x-ratelimit-reset` header.
    pub ratelimit_reset: Option<String>,
    /// The `ETag` header.
    pub etag: Option<String>,
}

/// The request didn't reach GitHub. Each kind carries the whole cause, for the log: a proxy that inspects
/// TLS, for example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreachable {
    /// No network, no answer in time, an address that isn't GitHub's, or a connection that failed some other
    /// way, a certificate that failed for a reason other than its dates among them.
    Offline(String),
    /// The crate refused to send the request, which would fail the same way each time it was asked: a
    /// GraphQL body that isn't a read, or a write's selection that could add to the write.
    Refused(String),
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

#[cfg(any(test, feature = "testing"))]
pub mod testing;
