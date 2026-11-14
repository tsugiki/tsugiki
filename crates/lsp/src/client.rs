//! One language server: the thread that owns it, and the handles the caller asks it through.
//!
//! The owner thread is the only one that knows which requests wait for an answer, and the only one that
//! sends the server messages. A writer thread writes them, so a server that stops reading holds up nobody
//! else, and two more threads read the server's output and its error output and send what they read to the
//! owner as messages, as the caller's handles do.

use std::collections::BTreeMap;
use std::convert::identity;
use std::fmt;
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::framing;
use crate::rpc::{
    self, INVALID_PARAMS, Incoming, METHOD_NOT_FOUND, ReplyError, RequestId, ResponseError,
};
use crate::types::{
    ConfigurationParams, Definitions, Diagnostics, InitializeResult, Language, Position, Progress,
    ServerInfo, Status, Uri,
};

#[cfg(test)]
mod tests;

/// The most of a server's error output an [`Exit`] keeps, from its end.
const TAIL_BYTES: usize = 8 * 1024;
/// The longest the end of a server's output waits for its error output to close too. A process the server
/// started can hold that pipe open after the server has gone.
const ERRORS_PATIENCE: Duration = Duration::from_secs(1);

/// A server's standard streams and its process. [`Server::start`] spawns one, and tests make one from
/// in-memory pipes and a fake process.
pub struct Streams {
    /// The server's standard input.
    pub input: Box<dyn Write + Send>,
    /// The server's standard output, which carries its messages.
    pub output: Box<dyn Read + Send>,
    /// The server's error output, which is its log.
    pub errors: Box<dyn Read + Send>,
    pub process: Box<dyn Process>,
}

/// The server's process, which the owner thread waits for and kills.
pub trait Process: Send {
    /// Waits for the process to end, and gives its exit code if it had one.
    fn wait(&mut self) -> Option<i32>;
    /// Ends the process now.
    fn kill(&mut self);
}

/// The workspace and settings the client gives a server with `initialize`.
#[derive(Clone, Debug)]
pub struct Init {
    /// The folder the server works in.
    pub root: Uri,
    /// The server's own `initializationOptions`.
    pub options: Value,
    /// The settings the client answers `workspace/configuration` from: an object whose keys are sections, as
    /// `{"ty": {"environment": {...}}}`.
    pub settings: Value,
    /// What else the client can do that only this server knows of, in `capabilities.experimental`, as
    /// rust-analyzer's `serverStatusNotification`.
    pub experimental: Value,
}

/// A message a server sends the caller without being asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Progress(Progress),
    Diagnostics(Diagnostics),
    /// rust-analyzer's `experimental/serverStatus`, sent to a client that says it takes it.
    Status(Status),
    /// The server's process ended, whether asked to or not. Nothing more comes from it.
    Exited(Exit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    /// `None` when the process ended without one, as when it was killed off Windows.
    pub code: Option<i32>,
    /// The end of what the server wrote to its error output.
    pub stderr_tail: String,
}

/// The reason a request got no result.
#[derive(Debug)]
pub enum RequestError {
    /// The server answered with an error.
    Server { code: i64, message: String },
    /// The caller cancelled it.
    Cancelled,
    /// The server's answer isn't what LSP says it should be.
    Unparsable(serde_json::Error),
    /// The server ended, or stopped reading, before it answered.
    Exited,
    /// The server didn't answer `initialize` in the time the caller gave it.
    TimedOut,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server { code, message } => {
                write!(f, "the server answered error {code}: {message}")
            }
            Self::Cancelled => f.write_str("the request was cancelled"),
            Self::Unparsable(cause) => write!(f, "the server's answer doesn't parse: {cause}"),
            Self::Exited => f.write_str("the server ended before it answered"),
            Self::TimedOut => f.write_str("the server didn't answer in time"),
        }
    }
}

impl std::error::Error for RequestError {}

/// The methods the client sends.
#[derive(Clone, Copy, Debug)]
enum Method {
    Initialize,
    Initialized,
    DidOpen,
    DidChange,
    DidClose,
    Definition,
    Shutdown,
    Exit,
    CancelRequest,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Initialized => "initialized",
            Self::DidOpen => "textDocument/didOpen",
            Self::DidChange => "textDocument/didChange",
            Self::DidClose => "textDocument/didClose",
            Self::Definition => "textDocument/definition",
            Self::Shutdown => "shutdown",
            Self::Exit => "exit",
            Self::CancelRequest => "$/cancelRequest",
        }
    }
}

/// Hands a request's outcome, still JSON, to whoever waits for it, parsed into what it asked for.
type Answer = Box<dyn FnOnce(Result<Value, RequestError>) + Send>;

/// A request's method and params, and what follows its answer.
struct Call {
    method: Method,
    params: Value,
    then: Option<Then>,
}

/// A request on its way to the owner thread.
struct Ask {
    id: i64,
    call: Call,
    answer: Answer,
}

/// A notification the client sends once the server has answered a request without an error.
#[derive(Clone, Copy)]
enum Then {
    /// After `initialize`.
    Initialized,
    /// After `shutdown`.
    Exit,
}

/// The messages the owner thread acts on, from the caller's handles and from the threads that read and
/// write the server.
enum Input {
    Ask(Ask),
    Cancel(i64),
    Notify {
        method: Method,
        params: Value,
    },
    /// The folder and settings the client answers the server's requests from, which come ahead of
    /// `initialize`, so a server that asks while it starts has its answer.
    Configure {
        root: Uri,
        settings: Value,
    },
    Arrived(Incoming),
    /// A piece of the server's error output.
    Errors(Vec<u8>),
    /// The server stopped reading what the client writes.
    StoppedReading,
    /// The server's output closed, and its error output has closed too or had its chance to.
    OutputEnded,
    /// The caller asked the server to stop, and gives it this long to agree, and as long again to exit.
    Stop(Duration),
    /// The caller let go of the [`Server`].
    Dropped,
}

/// Sends the owner thread its messages.
#[derive(Clone)]
struct ToOwner(Sender<Input>);

impl ToOwner {
    fn send(&self, input: Input) {
        // The owner ends only once the `Server` has dropped, and then nothing is left to do for this message.
        let _gone = self.0.send(input);
    }
}

/// A server that has started and not yet been initialized. LSP lets nothing reach a server before its
/// answer to `initialize`, so this offers nothing else, and dropping it kills the server.
pub struct Starting(Server);

impl Starting {
    /// Asks the server to start, waits up to `patience` for its answer, and sends `initialized`. The server
    /// can then be asked anything. The answer names the server, if it says.
    pub fn initialize(
        self,
        init: Init,
        patience: Duration,
    ) -> Result<(Server, Option<ServerInfo>), RequestError> {
        let Init {
            root,
            options,
            settings,
            experimental,
        } = init;
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "Tsugiki", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root,
            "workspaceFolders": [{"uri": root, "name": root}],
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "workspace": {"configuration": true, "workspaceFolders": true},
                "textDocument": {
                    "synchronization": {"dynamicRegistration": false},
                    "definition": {"dynamicRegistration": false, "linkSupport": true},
                    "publishDiagnostics": {"versionSupport": true},
                },
                "window": {"workDoneProgress": true},
                "experimental": experimental,
            },
            "initializationOptions": options,
        });
        self.0.inputs.send(Input::Configure { root, settings });
        let call = Call {
            method: Method::Initialize,
            params,
            then: Some(Then::Initialized),
        };
        let pending = self
            .0
            .ask(call, |result: InitializeResult| result.server_info);
        match pending.wait_timeout(patience) {
            Some(Ok(info)) => Ok((self.0, info)),
            Some(Err(error)) => Err(error),
            None => Err(RequestError::TimedOut),
        }
    }
}

/// A running language server. Dropping it kills the server's process: [`Server::stop`] lets the server end
/// on its own first.
pub struct Server {
    inputs: ToOwner,
    next_id: AtomicI64,
}

impl Server {
    /// Talks to a server over `streams`, and gives the events it sends.
    pub fn over(streams: Streams) -> (Starting, Receiver<Event>) {
        let (inputs, received) = mpsc::channel();
        let inputs = ToOwner(inputs);
        let (events, given) = mpsc::channel();
        let Streams {
            input,
            output,
            errors,
            process,
        } = streams;
        let (errors_open, errors_closed) = mpsc::channel::<()>();
        let reads = inputs.clone();
        let _errors = thread::spawn(move || read_errors(errors, &reads, errors_open));
        let reads = inputs.clone();
        let _output = thread::spawn(move || read_output(output, &errors_closed, &reads));
        let (writes, bodies) = mpsc::channel();
        let writer = inputs.clone();
        let _input = thread::spawn(move || write_input(input, &bodies, &writer));
        let owner = Owner {
            writes,
            process,
            events,
            waiting: BTreeMap::new(),
            root: None,
            settings: Value::Null,
            tail: Vec::new(),
            life: Life::Running,
        };
        let _owner = thread::spawn(move || owner.run(&received));
        (
            Starting(Self {
                inputs,
                next_id: AtomicI64::new(1),
            }),
            given,
        )
    }

    /// Opens a document with `text`, which the server then reads in place of the file on disk until the
    /// [`Document`] is dropped.
    pub fn open(&self, uri: Uri, language: Language, text: &str) -> Document {
        let params = json!({
            "textDocument": {"uri": uri, "languageId": language.id(), "version": 1, "text": text},
        });
        self.inputs.send(Input::Notify {
            method: Method::DidOpen,
            params,
        });
        Document {
            uri,
            version: 1,
            inputs: self.inputs.clone(),
        }
    }

    /// Asks where the name at `at` in `uri` is defined.
    pub fn definition(&self, uri: &Uri, at: Position) -> Pending<Definitions> {
        let params = json!({"textDocument": {"uri": uri}, "position": at});
        self.ask(
            Call {
                method: Method::Definition,
                params,
                then: None,
            },
            identity,
        )
    }

    /// Asks the server to stop with `shutdown`, sends `exit` when it agrees, and returns at once. A server
    /// that hasn't agreed within `patience`, or hasn't exited `patience` after `exit`, is killed. An
    /// [`Event::Exited`] follows if it exits on its own, and the events end either way.
    pub fn stop(self, patience: Duration) {
        let call = Call {
            method: Method::Shutdown,
            params: Value::Null,
            then: Some(Then::Exit),
        };
        let _agreed: Pending<()> = self.ask(call, identity);
        self.inputs.send(Input::Stop(patience));
    }

    /// Sends a request whose answer parses as a `W`, and hands the caller `convert` of it.
    fn ask<W: DeserializeOwned + 'static, T: Send + 'static>(
        &self,
        call: Call,
        convert: fn(W) -> T,
    ) -> Pending<T> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, outcome) = mpsc::sync_channel(1);
        let answer: Answer = Box::new(move |result| {
            let parsed = result.and_then(|value| {
                serde_json::from_value(value)
                    .map(convert)
                    .map_err(RequestError::Unparsable)
            });
            // The caller may have stopped waiting.
            let _unheard = sender.send(parsed);
        });
        self.inputs.send(Input::Ask(Ask { id, call, answer }));
        Pending {
            id,
            outcome,
            inputs: self.inputs.clone(),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.inputs.send(Input::Dropped);
    }
}

/// A request that the server hasn't answered yet.
pub struct Pending<T> {
    id: i64,
    outcome: Receiver<Result<T, RequestError>>,
    inputs: ToOwner,
}

impl<T> Pending<T> {
    /// Waits for the answer.
    pub fn wait(self) -> Result<T, RequestError> {
        self.outcome.recv().unwrap_or(Err(RequestError::Exited))
    }

    /// Waits up to `patience` for the answer, and gives `None` if it hasn't come.
    pub fn wait_timeout(&self, patience: Duration) -> Option<Result<T, RequestError>> {
        match self.outcome.recv_timeout(patience) {
            Ok(outcome) => Some(outcome),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(RequestError::Exited)),
        }
    }

    /// Tells the server to stop working on the request. The request then fails with
    /// [`RequestError::Cancelled`], unless its answer came first.
    pub fn cancel(&self) {
        self.canceller().cancel();
    }

    /// A handle that cancels the request, which another thread can keep while one waits for the answer.
    pub fn canceller(&self) -> Canceller {
        Canceller {
            id: self.id,
            inputs: self.inputs.clone(),
        }
    }
}

/// Cancels a request, as [`Pending::cancel`] does, from a thread that doesn't hold its [`Pending`].
#[derive(Clone)]
pub struct Canceller {
    id: i64,
    inputs: ToOwner,
}

impl Canceller {
    /// Tells the server to stop working on the request, unless its answer came first.
    pub fn cancel(&self) {
        self.inputs.send(Input::Cancel(self.id));
    }
}

/// A document open on the server, whose text the server reads in place of the file's on disk. Dropping it
/// closes it.
pub struct Document {
    uri: Uri,
    version: i32,
    inputs: ToOwner,
}

impl Document {
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// The version the server last got, which [`Diagnostics::version`] names.
    pub fn version(&self) -> i32 {
        self.version
    }

    /// Replaces the document's whole text.
    pub fn change(&mut self, text: &str) {
        self.version = self.version.saturating_add(1);
        let params = json!({
            "textDocument": {"uri": self.uri, "version": self.version},
            "contentChanges": [{"text": text}],
        });
        self.inputs.send(Input::Notify {
            method: Method::DidChange,
            params,
        });
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        let params = json!({"textDocument": {"uri": self.uri}});
        self.inputs.send(Input::Notify {
            method: Method::DidClose,
            params,
        });
    }
}

/// A request the server hasn't answered.
struct Waiting {
    answer: Answer,
    then: Option<Then>,
}

/// How far along the server is towards its end.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Life {
    /// The server reads what the client writes.
    Running,
    /// The client has sent `exit`, so the server is on its way out.
    Exiting,
    /// The server stopped reading, so nothing more can be asked, and its process is still there.
    Deaf,
    /// The server's output ended, and the caller has heard how its process ended.
    Exited,
}

/// The owner thread's state.
struct Owner {
    /// What the writer thread writes to the server.
    writes: Sender<Vec<u8>>,
    process: Box<dyn Process>,
    events: Sender<Event>,
    /// By request id.
    waiting: BTreeMap<i64, Waiting>,
    /// The folder of the [`Init`], which `workspace/workspaceFolders` asks for.
    root: Option<Uri>,
    settings: Value,
    /// The end of the server's error output, at most [`TAIL_BYTES`].
    tail: Vec<u8>,
    life: Life,
}

impl Owner {
    fn run(mut self, inputs: &Receiver<Input>) {
        while let Ok(input) = inputs.recv() {
            match input {
                Input::Ask(ask) => self.ask(ask),
                Input::Cancel(id) => self.cancel(id),
                Input::Notify { method, params } => {
                    self.write(rpc::notification(method.name(), params));
                }
                Input::Configure { root, settings } => {
                    self.root = Some(root);
                    self.settings = settings;
                }
                Input::Arrived(incoming) => self.arrived(incoming),
                Input::Errors(bytes) => self.keep_tail(&bytes),
                Input::StoppedReading => self.input_closed(),
                Input::OutputEnded => self.output_ended(),
                Input::Stop(patience) => {
                    self.await_exit(patience, inputs);
                    break;
                }
                Input::Dropped => break,
            }
        }
        // Dropping each answer unanswered tells whoever waits that the server has gone, before the kill.
        self.waiting.clear();
        if self.life != Life::Exited {
            self.process.kill();
            let _code = self.process.wait();
        }
    }

    /// Gives a server asked to stop `patience` to agree, and `patience` again after `exit` to end. On the way
    /// it keeps what the server says and answers what it asks, and writes nothing else to it.
    fn await_exit(&mut self, patience: Duration, inputs: &Receiver<Input>) {
        if self.life == Life::Exited {
            return;
        }
        let mut deadline = Instant::now() + patience;
        let mut exiting = false;
        while let Ok(input) =
            inputs.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            match input {
                Input::Arrived(incoming) => self.arrived(incoming),
                Input::Errors(bytes) => self.keep_tail(&bytes),
                Input::StoppedReading => self.input_closed(),
                Input::OutputEnded => {
                    self.output_ended();
                    return;
                }
                // The caller has let go of the server, and of its documents on the way out.
                Input::Ask(_)
                | Input::Cancel(_)
                | Input::Notify { .. }
                | Input::Configure { .. }
                | Input::Stop(_)
                | Input::Dropped => {}
            }
            if !exiting && self.life == Life::Exiting {
                exiting = true;
                deadline = Instant::now() + patience;
            }
        }
    }

    fn ask(&mut self, ask: Ask) {
        let Ask {
            id,
            call:
                Call {
                    method,
                    params,
                    then,
                },
            answer,
        } = ask;
        if self.life == Life::Running {
            self.write(rpc::request(id, method.name(), params));
            let _none = self.waiting.insert(id, Waiting { answer, then });
        } else {
            answer(Err(RequestError::Exited));
        }
    }

    fn cancel(&mut self, id: i64) {
        if let Some(waiting) = self.waiting.remove(&id) {
            (waiting.answer)(Err(RequestError::Cancelled));
            self.write(rpc::notification(
                Method::CancelRequest.name(),
                json!({"id": id}),
            ));
        }
    }

    /// Hands one message to the writer thread, which says if the server has stopped reading.
    fn write(&self, body: Vec<u8>) {
        // The writer has gone only after it said the server stopped reading.
        let _gone = self.writes.send(body);
    }

    fn arrived(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Reply { id, outcome } => self.answered(&id, outcome),
            Incoming::Request { id, method, params } => {
                let outcome = self.answer(&method, params);
                self.write(rpc::reply(&id, outcome));
            }
            Incoming::Notification { method, params } => self.notified(&method, params),
        }
    }

    fn answered(&mut self, id: &RequestId, outcome: Result<Value, ReplyError>) {
        let waiting = match id {
            RequestId::Number(number) => self.waiting.remove(number),
            RequestId::Text(_) => None,
        };
        let Some(Waiting { answer, then }) = waiting else {
            log::debug!("A language server answered request {id:?}, which nothing waits for");
            return;
        };
        match outcome {
            Ok(result) => {
                match then {
                    Some(Then::Initialized) => {
                        self.write(rpc::notification(Method::Initialized.name(), json!({})));
                    }
                    Some(Then::Exit) => {
                        self.write(rpc::notification(Method::Exit.name(), Value::Null));
                        self.life = Life::Exiting;
                    }
                    None => {}
                }
                answer(Ok(result));
            }
            Err(ReplyError::Server(ResponseError { code, message })) => {
                answer(Err(RequestError::Server { code, message }));
            }
            Err(ReplyError::Malformed(cause)) => {
                answer(Err(RequestError::Unparsable(serde::de::Error::custom(
                    cause,
                ))));
            }
        }
    }

    /// The client's answer to a request from the server.
    fn answer(&self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "workspace/configuration" => {
                match serde_json::from_value::<ConfigurationParams>(params) {
                    Ok(asked) => Ok(asked
                        .items
                        .iter()
                        .map(|item| self.section(item.section.as_deref()))
                        .collect()),
                    Err(cause) => Err(ResponseError {
                        code: INVALID_PARAMS,
                        message: cause.to_string(),
                    }),
                }
            }
            "workspace/workspaceFolders" => Ok(json!([{"uri": self.root, "name": self.root}])),
            // The client shows progress whoever starts it, and registers nothing.
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability" => Ok(Value::Null),
            other => Err(ResponseError {
                code: METHOD_NOT_FOUND,
                message: format!("Tsugiki doesn't answer {other}"),
            }),
        }
    }

    /// The settings at a dotted path such as `ty.environment`, or all of them for `None`.
    fn section(&self, path: Option<&str>) -> Value {
        let Some(path) = path else {
            return self.settings.clone();
        };
        path.split('.')
            .try_fold(&self.settings, |at, key| at.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn notified(&self, method: &str, params: Value) {
        let event = match method {
            "$/progress" => serde_json::from_value::<Progress>(params).map(Event::Progress),
            "textDocument/publishDiagnostics" => {
                serde_json::from_value::<Diagnostics>(params).map(Event::Diagnostics)
            }
            "experimental/serverStatus" => {
                serde_json::from_value::<Status>(params).map(Event::Status)
            }
            _ => return,
        };
        match event {
            // The caller may not listen.
            Ok(event) => drop(self.events.send(event)),
            Err(cause) => log::warn!("A language server's {method} doesn't parse: {cause}"),
        }
    }

    fn keep_tail(&mut self, bytes: &[u8]) {
        self.tail.extend_from_slice(bytes);
        let cut = self.tail.len().saturating_sub(TAIL_BYTES);
        if cut > 0 {
            self.tail = self.tail.split_off(cut);
        }
    }

    /// Fails what waits, as the server can't read what it asks.
    fn fail_waiting(&mut self) {
        for waiting in std::mem::take(&mut self.waiting).into_values() {
            (waiting.answer)(Err(RequestError::Exited));
        }
    }

    fn input_closed(&mut self) {
        self.fail_waiting();
        if self.life != Life::Exited {
            self.life = Life::Deaf;
        }
    }

    /// The server can't answer anything more, so what waits fails, and the caller hears how it ended.
    fn output_ended(&mut self) {
        self.fail_waiting();
        let code = self.process.wait();
        self.life = Life::Exited;
        let stderr_tail = String::from_utf8_lossy(&self.tail).into_owned();
        let _unheard = self.events.send(Event::Exited(Exit { code, stderr_tail }));
    }
}

/// Writes each message to the server until it stops reading, and then tells the owner.
fn write_input(mut input: Box<dyn Write + Send>, bodies: &Receiver<Vec<u8>>, inputs: &ToOwner) {
    for body in bodies {
        if let Err(cause) = framing::write(&mut input, &body) {
            log::warn!("Tsugiki can't write to a language server: {cause}");
            inputs.send(Input::StoppedReading);
            return;
        }
    }
}

/// Reads the server's messages until its output ends, then waits for its error output to end too, so the
/// owner has all of it when it hears the server has gone.
fn read_output(output: Box<dyn Read + Send>, errors_closed: &Receiver<()>, inputs: &ToOwner) {
    let mut reader = BufReader::new(output);
    loop {
        match framing::read(&mut reader) {
            Ok(Some(body)) => match rpc::parse(&body) {
                Ok(incoming) => inputs.send(Input::Arrived(incoming)),
                Err(cause) => log::warn!("A language server's message {cause}"),
            },
            Ok(None) => break,
            Err(cause) => {
                log::warn!("Tsugiki can't read a language server's output: {cause}");
                break;
            }
        }
    }
    // Closed, or held open past the patience, which the owner can't tell apart.
    let _closed_or_not = errors_closed.recv_timeout(ERRORS_PATIENCE);
    inputs.send(Input::OutputEnded);
}

/// Hands the owner the server's error output as it comes, and drops `open` when it ends.
fn read_errors(mut errors: Box<dyn Read + Send>, inputs: &ToOwner, open: Sender<()>) {
    let mut buffer = [0; 4096];
    while let Ok(read @ 1..) = errors.read(&mut buffer) {
        let piece = buffer.get(..read).unwrap_or_default().to_vec();
        inputs.send(Input::Errors(piece));
    }
    drop(open);
}
