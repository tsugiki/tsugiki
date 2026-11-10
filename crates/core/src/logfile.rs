//! A local log of why things failed, for a user reporting a bug (ADR 0008). Code anywhere in the core uses
//! the `log` crate's macros, and this is where those lines go.
//!
//! One thread owns the file and the tokens to hide, and every line goes through that thread, so nothing
//! reaches the file without scrubbing (ADR 0005). Callers don't wait on the disk (ADR 0017), and a line the
//! thread can't write is lost, because the log is the place Tsugiki would report that.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use log::{Level, LevelFilter, Log, Metadata, Record};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::version::Version;

/// At this size the file becomes `.1`, replacing the one before, so the two take about twice this.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_LINE_CHARS: usize = 4096;
/// How long `flush` waits for the thread, so a stuck disk can't hold up quitting.
const FLUSH_PATIENCE: Duration = Duration::from_secs(1);

const HIDDEN: &str = "[token]";
/// How GitHub's tokens start. `hide` covers a token in use whatever its shape.
const TOKEN_PREFIXES: [&str; 6] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];

enum Message {
    Line { when: String, text: String },
    Hide(String),
    Flush(Sender<()>),
}

struct Logger {
    messages: Sender<Message>,
}

impl Logger {
    fn start(path: PathBuf) -> Self {
        let (messages, queue) = mpsc::channel();
        let _ = thread::spawn(move || {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "writing a line fails too then, and the module doc says why that is dropped"
            )]
            let _ = fs::create_dir_all(path.with_file_name(""));
            let mut hidden = Vec::new();
            let mut last = String::new();
            for message in queue {
                match message {
                    // A repeat adds nothing, and a week offline would push older causes out of the file.
                    Message::Line { when, text } => {
                        let text = scrub(&text, &hidden);
                        if text != last {
                            #[expect(
                                clippy::let_underscore_must_use,
                                reason = "the module doc says why a line that can't be written is dropped"
                            )]
                            let _ = append(&path, &format!("{when} {text}"));
                            last = text;
                        }
                    }
                    Message::Hide(secret) => hidden.push(secret),
                    Message::Flush(done) => drop(done),
                }
            }
        });
        Self { messages }
    }

    fn send(&self, message: Message) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the thread has gone, and the log is where Tsugiki would say so"
        )]
        let _ = self.messages.send(message);
    }
}

impl Log for Logger {
    /// Tsugiki's own notes, and only what's wrong from the libraries, which say a lot more at lower levels.
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let lowest = if metadata.target().starts_with("tsugiki") {
            Level::Info
        } else {
            Level::Warn
        };
        metadata.level() <= lowest
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            let when = OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default();
            let text = format!("{} {}: {}", record.level(), record.target(), record.args());
            self.send(Message::Line { when, text });
        }
    }

    /// Waits for the lines sent so far to reach the file.
    fn flush(&self) {
        let (done, written) = mpsc::channel();
        self.send(Message::Flush(done));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "running out of patience only loses lines, as a crash would, and `done` going is the answer"
        )]
        let _ = written.recv_timeout(FLUSH_PATIENCE);
    }
}

/// Hides what's in `hidden`, and anything shaped like a token.
fn scrub(line: &str, hidden: &[String]) -> String {
    let without_known = hidden.iter().fold(line.to_owned(), |text, secret| {
        text.replace(secret.as_str(), HIDDEN)
    });
    TOKEN_PREFIXES.iter().fold(without_known, |text, prefix| {
        hide_after_prefix(&text, prefix)
    })
}

/// Hides each run of token characters that follows `prefix`. A prefix with nothing after it is only words.
fn hide_after_prefix(text: &str, prefix: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(prefix) {
        let tail = after.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        kept.push_str(before);
        kept.push_str(if tail.len() == after.len() {
            prefix
        } else {
            HIDDEN
        });
        rest = tail;
    }
    kept.push_str(rest);
    kept
}

/// Adds `line` to the file, moving the file aside first if it has grown too big. If something holds `.1` open
/// the file goes instead, because the newest lines are the ones a bug report needs.
fn append(path: &Path, line: &str) -> io::Result<()> {
    if fs::metadata(path).is_ok_and(|file| file.len() >= MAX_FILE_BYTES) {
        fs::rename(path, path.with_extension("log.1")).or_else(|_| fs::remove_file(path))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line: String = line.chars().take(MAX_LINE_CHARS).collect();
    file.write_all(format!("{line}\n").as_bytes())
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Sends the `log` crate's lines to the file at `path` from now on. Only the first call does anything.
pub fn start(path: PathBuf) {
    let logger = LOGGER.get_or_init(|| Logger::start(path));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

/// Marks where a run begins in the log. A second `tsg` starts the logger too, for its panics, but it hands
/// over to the first and exits without calling this.
pub fn say_started(version: &Version) {
    log::info!("Tsugiki {version} started");
}

/// Keeps `secret` out of the file, whatever its shape, for lines sent after this. It does nothing before
/// `start`, which is why the app starts the log before it reads a token.
pub(crate) fn hide(secret: &str) {
    if let Some(logger) = LOGGER.get() {
        logger.send(Message::Hide(secret.to_owned()));
    }
}

#[cfg(test)]
mod tests;
