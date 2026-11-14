//! A Repository with a PR, a Lookup checkout at its base as the Mirror worker leaves one, built with the
//! real git (ADR 0009), the servers the tests start, and a fake server for what a real one does only at
//! moments a test can't pick.

use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write, pipe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::{env, fs, thread};

use serde_json::{Value, json};
use tsugiki_lsp::{Event, Process, Server, Starting, Streams};
use tsugiki_test_git::git;

use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::testing::temp_path;

/// The ty that `TSUGIKI_TY` names, which `just cargo-test` and `just rust-coverage` set to the one
/// `scripts/test-servers.ts` fetches.
pub(crate) fn ty() -> PathBuf {
    let ty = PathBuf::from(env::var_os("TSUGIKI_TY").unwrap_or_default());
    assert!(ty.is_file(), "TSUGIKI_TY={ty:?}: see just test-servers");
    ty
}

/// The rust-analyzer that `TSUGIKI_RUST_ANALYZER` names, which `just cargo-test` and `just rust-coverage`
/// set to the one in the toolchain `rust-toolchain.toml` pins.
pub(crate) fn rust_analyzer() -> PathBuf {
    let path = PathBuf::from(env::var_os("TSUGIKI_RUST_ANALYZER").unwrap_or_default());
    assert!(
        path.is_file(),
        "TSUGIKI_RUST_ANALYZER={path:?}: see just test-servers"
    );
    path
}

/// The clangd that `TSUGIKI_CLANGD` names, which `just cargo-test` and `just rust-coverage` set to the one
/// `scripts/test-servers.ts` fetches.
pub(crate) fn clangd() -> PathBuf {
    let path = PathBuf::from(env::var_os("TSUGIKI_CLANGD").unwrap_or_default());
    assert!(
        path.is_file(),
        "TSUGIKI_CLANGD={path:?}: see just test-servers"
    );
    path
}

/// A repository whose `main` is the PR's base, and whose PR's head changes, adds and deletes files.
pub(crate) struct Fixture {
    /// The repository the PR is in, which stands in for its Mirror.
    pub(crate) mirror: PathBuf,
    pub(crate) checkout: LookupCheckout,
}

impl Fixture {
    /// The base holds `base`, and the PR's head writes each of `head`'s files, or deletes it where its text
    /// is None. The checkout holds the base and the head's added files, as a move leaves it.
    pub(crate) fn new(name: &str, base: &[(&str, &str)], head: &[(&str, Option<&str>)]) -> Self {
        // The folder keeps the `..` in `temp_path`'s path, which the core reads past.
        let root = temp_path(name);
        let mirror = root.join("mirror");
        fs::create_dir_all(&mirror).unwrap();
        let _ = git(&mirror, &["init", "-q", "-b", "main"]);
        let _ = git(&mirror, &["config", "core.autocrlf", "false"]);
        for (path, text) in base {
            write(&mirror, path, text);
        }
        let base_commit = commit(&mirror, "base");
        let mut added = Vec::new();
        for (path, text) in head {
            match text {
                Some(text) => {
                    if !mirror.join(path).exists() {
                        added.push(*path);
                    }
                    write(&mirror, path, text);
                }
                None => {
                    let _ = git(&mirror, &["rm", "-q", path]);
                }
            }
        }
        let head_commit = commit(&mirror, "head");
        let folder = root.join("lookup");
        let folder_arg = folder.to_string_lossy().into_owned();
        let at = [
            "worktree",
            "add",
            "-q",
            "--detach",
            &folder_arg,
            base_commit.as_str(),
        ];
        let _ = git(&mirror, &at);
        if !added.is_empty() {
            let mut args = vec!["checkout", head_commit.as_str(), "--"];
            args.extend(added);
            let _ = git(&folder, &args);
        }
        Self {
            mirror,
            checkout: LookupCheckout {
                folder,
                at: LookupAt {
                    base: base_commit,
                    head: head_commit,
                },
            },
        }
    }
}

fn write(folder: &Path, path: &str, text: &str) {
    let file = folder.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, text).unwrap();
    let _ = git(folder, &["add", path]);
}

fn commit(folder: &Path, message: &str) -> CommitSha {
    let _ = git(folder, &["commit", "-q", "--allow-empty", "-m", message]);
    let commit = git(folder, &["rev-parse", "HEAD"]);
    CommitSha::parse(commit.trim()).unwrap()
}

/// A fake clangd, which threads of the test play over in-memory pipes: it answers each lookup with the
/// locations it was given, and reports its index's progress when the test says. The real clangd indexes a
/// small Repository before a test could ask it anything.
pub(crate) struct FakeClangd {
    /// The messages the fake writes to the client, in order, and `None` to end its output as a crash does.
    says: Sender<Option<Value>>,
    /// The lookups the client has asked it.
    asked: Arc<AtomicUsize>,
}

impl FakeClangd {
    /// A fake clangd that answers the client's first `textDocument/definition` with the first of `answers`,
    /// the next with the next, and the rest with the last, and the client that talks to it, which has yet
    /// to initialize it. An answer with an `error` field fails its request with that error.
    pub(crate) fn start(answers: Vec<Value>) -> (Self, (Starting, Receiver<Event>)) {
        let (client_reads, server_writes) = pipe().unwrap();
        let (server_reads, client_writes) = pipe().unwrap();
        let (errors_read, errors_written) = pipe().unwrap();
        let (says, said) = mpsc::channel();
        let (ended, end) = mpsc::channel::<()>();
        let crashed = ended.clone();
        let _writes = thread::spawn(move || {
            write_messages(server_writes, &said);
            let _gone = crashed.send(());
        });
        let (replies, asked) = (says.clone(), Arc::new(AtomicUsize::new(0)));
        let counts = Arc::clone(&asked);
        let _reads = thread::spawn(move || {
            serve(server_reads, &replies, (&answers, &counts));
            // The process ends, and its error output with it, once it stops reading.
            let _gone = ended.send(());
            drop(errors_written);
        });
        let client = Server::over(Streams {
            input: Box::new(client_writes),
            output: Box::new(client_reads),
            errors: Box::new(errors_read),
            process: Box::new(FakeProcess(end)),
        });
        (Self { says, asked }, client)
    }

    /// Reports clangd's background index's progress as `value`, the value of a `$/progress`.
    pub(crate) fn progress(&self, value: Value) {
        let params = json!({"token": "backgroundIndexProgress", "value": value});
        let progress = json!({"jsonrpc": "2.0", "method": "$/progress", "params": params});
        self.says.send(Some(progress)).unwrap();
    }

    /// Ends the fake's output and its process, as a crash does.
    pub(crate) fn crash(&self) {
        self.says.send(None).unwrap();
    }

    /// The lookups the client has asked the fake.
    pub(crate) fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

/// The process a [`FakeClangd`] stands in for, which ends once the fake stops reading or crashes.
struct FakeProcess(Receiver<()>);

impl Process for FakeProcess {
    fn wait(&mut self) -> Option<i32> {
        let _ended = self.0.recv();
        Some(0)
    }

    fn kill(&mut self) {}
}

/// Answers each request the client writes to `input` until it sends `exit` or closes the input: each
/// `textDocument/definition` with the next of `answers`, or the last, counting them in `asked`, and the rest
/// with an empty result.
fn serve(
    input: PipeReader,
    replies: &Sender<Option<Value>>,
    (answers, asked): (&[Value], &AtomicUsize),
) {
    let mut input = BufReader::new(input);
    while let Some(message) = read_message(&mut input) {
        let (Some(method), id) = (message["method"].as_str(), message.get("id")) else {
            continue;
        };
        let result = match method {
            "exit" => return,
            "initialize" => json!({"capabilities": {}}),
            "textDocument/definition" => {
                let count = asked.fetch_add(1, Ordering::SeqCst);
                answers[count.min(answers.len() - 1)].clone()
            }
            _ => Value::Null,
        };
        // A notification gets no answer, and an answer with an `error` fails its request.
        if let Some(id) = id {
            let reply = match result.get("error") {
                Some(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
                None => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            };
            let _gone = replies.send(Some(reply));
        }
    }
}

/// The next message on `input`, or `None` once it has ended.
fn read_message(input: &mut impl BufRead) -> Option<Value> {
    // The client writes one header, `Content-Length`, then a blank line.
    let mut header = String::new();
    let _read = input.read_line(&mut header).ok()?;
    let length = header
        .strip_prefix("Content-Length:")?
        .trim()
        .parse()
        .ok()?;
    let mut blank = String::new();
    let _read = input.read_line(&mut blank).ok()?;
    let mut body = vec![0; length];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// Writes each message `said` gives to `output`, framed, until the client stops reading.
fn write_messages(mut output: PipeWriter, said: &Receiver<Option<Value>>) {
    // A `None` ends the output.
    while let Ok(Some(message)) = said.recv() {
        let body = message.to_string();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let written = (output.write_all(header.as_bytes()))
            .and_then(|()| output.write_all(body.as_bytes()))
            .and_then(|()| output.flush());
        if written.is_err() {
            return;
        }
    }
}
