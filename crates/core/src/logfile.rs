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
        log::info!("Tsugiki {} started", env!("CARGO_PKG_VERSION"));
    }
}

/// Keeps `secret` out of the file, whatever its shape, for lines sent after this. It does nothing before
/// `start`, which is why the app starts the log before it reads a token.
pub(crate) fn hide(secret: &str) {
    if let Some(logger) = LOGGER.get() {
        logger.send(Message::Hide(secret.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A logger with its own folder, so tests don't share the process's one logger.
    fn temp_logger(name: &str) -> (Logger, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("tsugiki-log-{name}-{}", std::process::id()));
        let logger = Logger::start(folder.join("tsugiki.log"));
        (logger, folder)
    }

    fn say(logger: &Logger, level: Level, target: &str, text: &str) {
        logger.log(
            &Record::builder()
                .args(format_args!("{text}"))
                .level(level)
                .target(target)
                .build(),
        );
    }

    /// What reached the file, once the thread has written everything, however long that takes.
    fn written(logger: &Logger, folder: &Path) -> String {
        let (done, reached) = mpsc::channel::<()>();
        logger.send(Message::Flush(done));
        assert!(reached.recv().is_err(), "the thread drops `done`");
        fs::read_to_string(folder.join("tsugiki.log")).unwrap()
    }

    #[test]
    fn a_line_says_when_how_bad_where_and_what() {
        let (logger, folder) = temp_logger("line");
        say(
            &logger,
            Level::Warn,
            "tsugiki_core::store",
            "the disk is full",
        );

        let file = written(&logger, &folder);

        assert!(
            file.contains(" WARN tsugiki_core::store: the disk is full\n"),
            "{file}"
        );
        assert!(file.starts_with("20"), "it starts with the year: {file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_token_in_use_never_reaches_the_file_whatever_its_shape() {
        let (logger, folder) = temp_logger("in-use");
        logger.send(Message::Hide("0123456789abcdef".to_owned()));
        say(
            &logger,
            Level::Warn,
            "tsugiki",
            "Bearer 0123456789abcdef refused",
        );

        let file = written(&logger, &folder);

        assert!(!file.contains("0123456789abcdef"), "{file}");
        assert!(file.contains("Bearer [token] refused"), "{file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn text_shaped_like_a_token_never_reaches_the_file() {
        let (logger, folder) = temp_logger("shaped");
        for token in [
            "ghp_abc123",
            "gho_abc123",
            "ghu_abc123",
            "ghs_abc123",
            "ghr_abc123",
            "github_pat_11ABC_def",
        ] {
            say(
                &logger,
                Level::Warn,
                "tsugiki",
                &format!("first {token}, then {token}."),
            );
        }

        let file = written(&logger, &folder);

        assert!(!file.contains("abc123"), "{file}");
        assert!(!file.contains("11ABC"), "{file}");
        assert!(file.contains("first [token], then [token]."), "{file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_prefix_with_nothing_after_it_is_only_words() {
        let (logger, folder) = temp_logger("prefix");
        say(
            &logger,
            Level::Warn,
            "tsugiki",
            "tokens start with ghp_ or gho_",
        );

        let file = written(&logger, &folder);

        assert!(file.contains("tokens start with ghp_ or gho_\n"), "{file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_token_cut_off_by_the_length_limit_still_doesnt_show() {
        let (logger, folder) = temp_logger("cut");
        let token = format!("ghp_{}", "a".repeat(MAX_LINE_CHARS * 2));
        say(&logger, Level::Warn, "tsugiki", &token);

        let file = written(&logger, &folder);

        assert!(!file.contains("aaaa"), "{file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_long_line_is_cut_on_a_character_and_the_next_line_still_starts_fresh() {
        let (logger, folder) = temp_logger("long");
        say(
            &logger,
            Level::Warn,
            "tsugiki",
            &"é".repeat(MAX_LINE_CHARS * 2),
        );
        say(&logger, Level::Warn, "tsugiki", "the next one");

        let file = written(&logger, &folder);

        assert!(
            file.chars().count() < MAX_LINE_CHARS + 100,
            "the long line was cut"
        );
        assert!(
            file.ends_with("tsugiki: the next one\n"),
            "the next line follows"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    fn file_size(path: &Path) -> u64 {
        fs::metadata(path).unwrap().len()
    }

    #[test]
    fn the_file_and_the_one_before_it_stay_under_the_limit() {
        let (logger, folder) = temp_logger("bounded");
        let line = "x".repeat(MAX_LINE_CHARS);
        let lines_for_three_files = usize::try_from(MAX_FILE_BYTES).unwrap() * 3 / MAX_LINE_CHARS;
        for number in 0..lines_for_three_files {
            say(&logger, Level::Warn, "tsugiki", &format!("{number} {line}"));
        }
        say(&logger, Level::Warn, "tsugiki", "the last one");

        let file = written(&logger, &folder);

        assert!(file.ends_with("tsugiki: the last one\n"));
        let limit = MAX_FILE_BYTES + u64::try_from(MAX_LINE_CHARS).unwrap();
        assert!(file_size(&folder.join("tsugiki.log")) <= limit);
        assert!(file_size(&folder.join("tsugiki.log.1")) <= limit);
        assert_eq!(
            fs::read_dir(&folder).unwrap().count(),
            2,
            "and no third file"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_that_cant_be_moved_aside_makes_way_for_new_lines() {
        let (logger, folder) = temp_logger("cant-move");
        fs::create_dir_all(folder.join("tsugiki.log.1")).unwrap();
        let full = "x".repeat(usize::try_from(MAX_FILE_BYTES).unwrap());
        fs::write(folder.join("tsugiki.log"), &full).unwrap();

        say(&logger, Level::Warn, "tsugiki", "the newest line");

        let file = written(&logger, &folder);
        assert!(file.ends_with("tsugiki: the newest line\n"), "{file}");
        assert!(file.len() < 100, "the old lines made way");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_line_the_same_as_the_last_isnt_written_again() {
        let (logger, folder) = temp_logger("repeats");
        for text in ["offline", "offline", "offline", "a bad answer", "offline"] {
            say(&logger, Level::Warn, "tsugiki", text);
        }

        let file = written(&logger, &folder);

        let said: Vec<_> = file
            .lines()
            .map(|line| line.rsplit(": ").next().unwrap())
            .collect();
        assert_eq!(said, ["offline", "a bad answer", "offline"]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_folder_it_cant_create_loses_the_lines_and_nothing_else() {
        let blocker =
            std::env::temp_dir().join(format!("tsugiki-log-blocker-{}", std::process::id()));
        fs::write(&blocker, "a file where the folder should be").unwrap();
        let logger = Logger::start(blocker.join("tsugiki.log"));

        say(&logger, Level::Warn, "tsugiki", "this has nowhere to go");
        logger.flush();

        fs::remove_file(blocker).unwrap();
    }

    #[test]
    fn libraries_only_write_what_is_wrong() {
        let (logger, folder) = temp_logger("libraries");
        say(&logger, Level::Info, "ureq", "connected");
        say(&logger, Level::Warn, "rustls", "a bad certificate");
        say(&logger, Level::Info, "tsugiki_core", "started");
        say(&logger, Level::Debug, "tsugiki_core", "details");

        let file = written(&logger, &folder);

        assert!(!file.contains("connected"), "{file}");
        assert!(file.contains("a bad certificate"), "{file}");
        assert!(file.contains("started"), "{file}");
        assert!(!file.contains("details"), "{file}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn hiding_a_token_before_the_log_starts_does_nothing() {
        // The process's logger starts only in `tests/logfile.rs`, which has a process of its own.
        hide("nothing is listening");
        assert!(LOGGER.get().is_none());
    }
}
