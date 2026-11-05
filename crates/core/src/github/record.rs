//! Captures real answers as fixtures for the replayer, with tokens scrubbed and each person's login replaced
//! wherever an answer names them (ADR 0009).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::conventions::CPYTHON_BACKPORT_BOT;
use crate::token::Token;

const PREFIXES: [&str; 6] = ["gho_", "ghp_", "ghs_", "ghu_", "ghr_", "github_pat_"];
const SCRUBBED: &str = "[token]";

/// Written in place of the viewer's login, so a fixture doesn't depend on who recorded it.
const VIEWER_STAND_IN: &str = "octocat";
/// Stands in for a description nothing reads, with the line breaks GitHub's own answers have.
const FILLER: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\r\n\r\n";
/// The markers of a line of a description that names a person, with their email.
const PEOPLE_LINES: [&str; 2] = ["co-authored-by:", "signed-off-by:"];
/// The one owner a recording's Repository names stay under. An answer also names other Repositories, such
/// as a fork that mentions an Issue, whose owner is a person.
const RECORDED_OWNER: &str = "python";
/// The start of the one line Tsugiki reads in a commit message.
const CHERRY_PICK: &str = "(cherry picked from commit ";
/// The fields of an answer that people write, where a login can sit inside the text.
const PROSE_FIELDS: [&str; 2] = ["title", "body"];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// What follows the token that `text` starts with, if it does.
fn after_token(text: &str) -> Option<&str> {
    PREFIXES.iter().find_map(|prefix| {
        let tail = text.strip_prefix(prefix)?;
        let after = tail.trim_start_matches(is_token_char);
        (after.len() < tail.len()).then_some(after)
    })
}

/// Removes `token`, and anything else shaped like a GitHub token, wherever it starts: the text is raw
/// JSON, so a token can follow an escape such as `\n`.
pub fn scrub(text: &str, token: &str) -> String {
    let text = text.replace(token, SCRUBBED);
    let mut scrubbed = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while !rest.is_empty() {
        if let Some(after) = after_token(rest) {
            scrubbed.push_str(SCRUBBED);
            rest = after;
        } else {
            let mut chars = rest.chars();
            scrubbed.extend(chars.next());
            rest = chars.as_str();
        }
    }
    scrubbed
}

/// Keeps a recording to what Tsugiki reads, because the rest can name people. A description on the default
/// branch becomes filler of the same length (ADR 0011), and the length keeps a recording as large as GitHub's
/// answer. A description off that branch loses the lines that credit a person, and a commit message keeps
/// only its cherry-pick lines.
fn keep_only_what_is_read(body: &mut Value) {
    if let Some(Value::Array(nodes)) = body.pointer_mut("/data/nodes") {
        nodes.iter_mut().for_each(keep_what_a_pr_reads);
    }
    if let Some(node) = body.pointer_mut("/data/repository/pullRequest") {
        keep_what_a_pr_reads(node);
    }
}

fn keep_what_a_pr_reads(node: &mut Value) {
    let base = node.pointer("/baseRefName");
    let on_default = base.is_some() && base == node.pointer("/repository/defaultBranchRef/name");
    if let Some(Value::String(description)) = node.get_mut("body") {
        *description = if on_default {
            FILLER
                .chars()
                .cycle()
                .take(description.chars().count())
                .collect()
        } else {
            without_lines(description, |line| {
                let line = line.to_ascii_lowercase();
                PEOPLE_LINES.iter().any(|marker| line.contains(marker))
            })
        };
    }
    if let Some(Value::Array(commits)) = node.pointer_mut("/commits/nodes") {
        for commit in commits {
            if let Some(Value::String(message)) = commit.pointer_mut("/commit/message") {
                *message = without_lines(message, |line| !line.starts_with(CHERRY_PICK));
            }
        }
    }
}

/// `text` without the lines `unwanted` picks, with its line breaks kept.
fn without_lines(text: &str, unwanted: impl Fn(&str) -> bool) -> String {
    text.split_inclusive('\n')
        .filter(|line| !unwanted(line.trim_start()))
        .collect()
}

/// True for a character a GitHub login can hold.
fn is_login_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// The logins a run of recordings has met, in lower case, and the stand-in for each. One person keeps one
/// stand-in across recordings, so a PR's author and the same person's Issue still match.
struct StandIns {
    by_login: BTreeMap<String, String>,
}

impl Default for StandIns {
    /// The backport bot stays as it is, because the Backport rules have to recognise it in a recording.
    fn default() -> Self {
        let bot = CPYTHON_BACKPORT_BOT.to_owned();
        Self {
            by_login: BTreeMap::from([(bot.clone(), bot)]),
        }
    }
}

impl StandIns {
    fn add_viewer(&mut self, login: &str) {
        drop(
            self.by_login
                .insert(login.to_ascii_lowercase(), VIEWER_STAND_IN.to_owned()),
        );
    }

    /// The stand-in for `login`, a new one the first time.
    fn for_login(&mut self, login: &str) -> String {
        // The bot's entry takes no number.
        let next = format!("user-{}", self.by_login.len().saturating_sub(1));
        self.by_login
            .entry(login.to_ascii_lowercase())
            .or_insert(next)
            .clone()
    }

    fn known(&self, word: &str) -> Option<&String> {
        self.by_login.get(&word.to_ascii_lowercase())
    }

    /// Replaces each `login` field, so the next pass knows everyone this answer names.
    fn replace_logins(&mut self, value: &mut Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.replace_logins(item);
                }
            }
            Value::Object(fields) => {
                for (name, field) in fields.iter_mut() {
                    if name == "login"
                        && let Value::String(login) = &mut *field
                    {
                        *login = self.for_login(login);
                    } else {
                        self.replace_logins(field);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    /// Replaces known logins in `value`'s other strings. Elsewhere only a whole string counts, because
    /// replacing part of one could change a node's id.
    fn replace_in_strings(&mut self, value: &mut Value) {
        match value {
            Value::String(text) => {
                if let Some(stand_in) = self.known(text) {
                    text.clone_from(stand_in);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.replace_in_strings(item);
                }
            }
            Value::Object(fields) => {
                for (name, field) in fields.iter_mut() {
                    if let Value::String(text) = &mut *field
                        && PROSE_FIELDS.contains(&name.as_str())
                    {
                        *text = self.in_prose(text);
                    } else if let Value::String(text) = &mut *field
                        && name == "nameWithOwner"
                    {
                        *text = self.in_repository_name(text);
                    } else {
                        self.replace_in_strings(field);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// `name` with an owner other than the recorded one replaced, because that owner is a person's login.
    fn in_repository_name(&mut self, name: &str) -> String {
        match name.split_once('/') {
            Some((owner, rest)) if !owner.eq_ignore_ascii_case(RECORDED_OWNER) => {
                format!("{}/{rest}", self.for_login(owner))
            }
            _ => name.to_owned(),
        }
    }

    /// `text` with each known login and each `@mention` replaced. A word with `@` right after another word
    /// is an email's domain, which names nobody. A real name written out in `text` stays, so read a new
    /// recording before you commit it.
    fn in_prose(&mut self, text: &str) -> String {
        let mut replaced = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(is_login_char) {
            let (before, tail) = rest.split_at(start);
            let end = tail.find(|c| !is_login_char(c)).unwrap_or(tail.len());
            let (word, after) = tail.split_at(end);
            replaced.push_str(before);
            let is_mention = before
                .strip_suffix('@')
                .is_some_and(|head| !head.is_empty() || replaced.len() == before.len());
            if is_mention {
                replaced.push_str(&self.for_login(word));
            } else {
                replaced.push_str(self.known(word).map_or(word, String::as_str));
            }
            rest = after;
        }
        replaced.push_str(rest);
        replaced
    }
}

/// Passes requests on to `inner`, and writes each good answer to `<dir>/<operation>.json`. A failure
/// keeps the recording that is there.
pub struct Recorder {
    inner: Box<dyn Transport>,
    dir: PathBuf,
    /// Later answers name the viewer as an author or a reviewer, with nothing to show that it's them, so
    /// the answer that says who they are adds them here.
    stand_ins: RefCell<StandIns>,
}

impl Recorder {
    pub fn new(inner: Box<dyn Transport>, dir: PathBuf) -> Self {
        Self {
            inner,
            dir,
            stand_ins: RefCell::default(),
        }
    }
}

impl Transport for Recorder {
    /// Passes the request on and records nothing, because recordings hold GraphQL only (ADR 0009).
    fn get(
        &self,
        token: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        self.inner.get(token, request)
    }

    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "a dev tool: a recording it can't write should stop it, and a failed request isn't one"
    )]
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let reply = self.inner.post(token, request)?;
        let text = scrub(&reply.body, token.expose());
        let mut body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        if reply.status != 200 || body.get("errors").is_some() {
            return Ok(reply);
        }
        keep_only_what_is_read(&mut body);
        let mut stand_ins = self.stand_ins.borrow_mut();
        if let Some(login) = body.pointer("/data/viewer/login").and_then(Value::as_str) {
            stand_ins.add_viewer(login);
        }
        stand_ins.replace_logins(&mut body);
        stand_ins.replace_in_strings(&mut body);
        let recording = json!({
            "query": request.query,
            "status": reply.status,
            "reset": reply.reset,
            "body": body,
        });
        let pretty =
            serde_json::to_string_pretty(&recording).expect("JSON values always serialize");
        std::fs::create_dir_all(&self.dir).expect("create the fixtures folder");
        let path = self.dir.join(format!("{}.json", request.operation));
        std::fs::write(path, pretty + "\n").expect("write the recording");
        Ok(reply)
    }
}

#[cfg(test)]
mod tests;
