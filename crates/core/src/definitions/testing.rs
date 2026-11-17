//! The servers the tests start, and a fake server for what a real one does only at moments a test can't
//! pick. The Repository with a PR that they work in is [`crate::testing::Fixture`].

use std::collections::HashSet;
use std::io::{BufRead, PipeWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};
use tsugiki_lsp::testing::{pipes, read_message, write_message};
use tsugiki_lsp::{Event, Starting, Uri};
/// The ty, rust-analyzer and clangd that `just cargo-test` and `just rust-coverage` name.
pub(crate) use tsugiki_test_git::{clangd, rust_analyzer, ty};

/// A fake clangd, which threads of the test play over in-memory pipes: it answers each lookup with the
/// locations it was given, and reports its index's progress when the test says. The real clangd indexes a
/// small Repository before a test could ask it anything. As clangd does, it refuses a lookup in a document
/// it wasn't sent.
pub(crate) struct FakeClangd {
    /// The messages the fake writes to the client, in order, and `None` to end its output as a crash does.
    says: Sender<Option<Value>>,
    /// The lookups the client has asked it.
    asked: Arc<AtomicUsize>,
    /// Each document the client has opened or closed on it, in order.
    documents: Arc<Mutex<Vec<Notified>>>,
    /// Its error output, until it ends.
    errors: Arc<Mutex<Option<PipeWriter>>>,
    /// The exit code its process ends with.
    code: Arc<AtomicI32>,
}

/// A document the client opened or closed on a [`FakeClangd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notified {
    Opened(Uri),
    Closed(Uri),
}

/// What a [`FakeClangd`] does with `initialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Initialize {
    Answer,
    /// Fails it with an error.
    Refuse,
    /// Answers it, then ends with exit code 0 once the client says it is initialized.
    AnswerThenLeave,
}

impl FakeClangd {
    /// A fake clangd that answers the client's first `textDocument/definition` with the first of `answers`,
    /// the next with the next, and the rest with the last, and the client that talks to it, which has yet
    /// to initialize it. An answer with an `error` field fails its request with that error, and one with a
    /// `silent` field never comes.
    pub(crate) fn start(answers: Vec<Value>) -> (Self, (Starting, Receiver<Event>)) {
        Self::starting(answers, Initialize::Answer)
    }

    /// [`FakeClangd::start`], with a fake that does what `initialize` says with the client's `initialize`.
    pub(crate) fn starting(
        answers: Vec<Value>,
        initialize: Initialize,
    ) -> (Self, (Starting, Receiver<Event>)) {
        let (starting, events, ends) = pipes(None);
        let errors = Arc::new(Mutex::new(Some(ends.errors)));
        let code = Arc::new(AtomicI32::new(0));
        let (says, said) = mpsc::channel();
        // The process ends with the code it holds once the fake stops writing, as a crash does, or reading.
        let (ended, crashed) = (
            Ended(ends.exit.clone(), Arc::clone(&code)),
            Ended(ends.exit, Arc::clone(&code)),
        );
        let mut writes = ends.writes;
        let _writes = thread::spawn(move || {
            write_messages(&mut writes, &said);
            crashed.end();
        });
        let (replies, asked) = (says.clone(), Arc::new(AtomicUsize::new(0)));
        let (counts, closes) = (Arc::clone(&asked), Arc::clone(&errors));
        let documents = Arc::new(Mutex::new(Vec::new()));
        let logs = Arc::clone(&documents);
        let mut reads = ends.reads;
        let _reads = thread::spawn(move || {
            serve(&mut reads, &replies, (&answers, &counts, &logs), initialize);
            // Its error output ends with it.
            ended.end();
            drop(closes.lock().unwrap().take());
        });
        let client = (starting, events);
        let fake = Self {
            says,
            asked,
            documents,
            errors,
            code,
        };
        (fake, client)
    }

    /// Reports clangd's background index's progress as `value`, the value of a `$/progress`.
    pub(crate) fn progress(&self, value: Value) {
        let params = json!({"token": "backgroundIndexProgress", "value": value});
        let progress = json!({"jsonrpc": "2.0", "method": "$/progress", "params": params});
        self.says.send(Some(progress)).unwrap();
    }

    /// Reports an error with `message` on line `line`, from 0, of the open file at `file`, as clangd reports
    /// a header it can't find.
    pub(crate) fn diagnose(&self, file: &Path, (line, message): (u32, &str)) {
        let start = json!({"line": line, "character": 9});
        let end = json!({"line": line, "character": 19});
        let diagnostic = json!({
            "range": {"start": start, "end": end},
            "severity": 1,
            "code": "pp_file_not_found",
            "source": "clang",
            "message": message,
        });
        let uri = Uri::from_path(file).unwrap();
        let params = json!({"uri": uri, "version": 1, "diagnostics": [diagnostic]});
        let method = "textDocument/publishDiagnostics";
        let diagnostics = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.says.send(Some(diagnostics)).unwrap();
    }

    /// Writes `stderr` to the fake's error output and ends it, then ends its output and its process with
    /// exit code `code`, as a crash does.
    pub(crate) fn crash(&self, code: i32, stderr: &str) {
        if let Some(mut errors) = self.errors.lock().unwrap().take() {
            errors.write_all(stderr.as_bytes()).unwrap();
        }
        self.code.store(code, Ordering::SeqCst);
        self.says.send(None).unwrap();
    }

    /// The lookups the client has asked the fake.
    pub(crate) fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// Each document the client has opened or closed on the fake, in order.
    pub(crate) fn documents(&self) -> Vec<Notified> {
        self.documents.lock().unwrap().clone()
    }
}

/// Ends the process a [`FakeClangd`] stands in for, with the exit code it holds then.
struct Ended(Sender<Option<i32>>, Arc<AtomicI32>);

impl Ended {
    fn end(&self) {
        let _gone = self.0.send(Some(self.1.load(Ordering::SeqCst)));
    }
}

/// Answers each request the client writes to `input` until it sends `exit` or closes the input: each
/// `textDocument/definition` with the next of `answers`, or the last, counting them in `asked`, and the rest
/// with an empty result. A lookup in a document the client hasn't opened fails, as clangd's does, and
/// `documents` logs each the client opens or closes. It does what `initialize` says with `initialize`.
fn serve(
    input: &mut impl BufRead,
    replies: &Sender<Option<Value>>,
    (answers, asked, documents): (&[Value], &AtomicUsize, &Mutex<Vec<Notified>>),
    initialize: Initialize,
) {
    let mut open = HashSet::new();
    while let Some(message) = read_message(input) {
        let (Some(method), id) = (message["method"].as_str(), message.get("id")) else {
            continue;
        };
        let document = message["params"]["textDocument"]["uri"].clone();
        let document = serde_json::from_value::<Uri>(document).ok();
        let result = match method {
            "exit" => return,
            "initialized" if initialize == Initialize::AnswerThenLeave => {
                let _gone = replies.send(None);
                return;
            }
            "initialize" if initialize == Initialize::Refuse => {
                json!({"error": {"code": -32603, "message": "the workspace can't be read"}})
            }
            "initialize" => json!({"capabilities": {}}),
            "textDocument/didOpen" => {
                let document = document.unwrap();
                let _new = open.insert(document.clone());
                documents.lock().unwrap().push(Notified::Opened(document));
                Value::Null
            }
            "textDocument/didClose" => {
                let document = document.unwrap();
                let _was = open.remove(&document);
                documents.lock().unwrap().push(Notified::Closed(document));
                Value::Null
            }
            "textDocument/definition" if !document.is_some_and(|uri| open.contains(&uri)) => {
                let message = "trying to get AST for non-added document";
                json!({"error": {"code": -32602, "message": message}})
            }
            "textDocument/definition" => {
                let count = asked.fetch_add(1, Ordering::SeqCst);
                answers[count.min(answers.len() - 1)].clone()
            }
            _ => Value::Null,
        };
        // A notification gets no answer, nor does a request whose answer is `silent`, and an answer with
        // an `error` fails its request.
        if let Some(id) = id.filter(|_| result.get("silent").is_none()) {
            let reply = match result.get("error") {
                Some(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
                None => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            };
            let _gone = replies.send(Some(reply));
        }
    }
}

/// Writes each message `said` gives to `output`, framed, until the client stops reading.
fn write_messages(output: &mut impl Write, said: &Receiver<Option<Value>>) {
    // A `None` ends the output.
    while let Ok(Some(message)) = said.recv() {
        if write_message(output, &message).is_err() {
            return;
        }
    }
}
