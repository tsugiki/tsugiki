//! A fake language server for the client's tests. The test's own thread plays the server, over in-memory
//! pipes, so each test decides what the server reads and when it answers.

use std::io::{BufReader, PipeReader, PipeWriter, Write, pipe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{Event, Init, Process, RequestError, Server, Starting, Streams};
use crate::framing;
use crate::types::{ServerInfo, Uri};

/// Longer than any of these tests should wait for the client.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// The server's side of the pipes, and its process's controls.
pub(crate) struct Fake {
    /// What the client writes. `None` once the server has stopped reading.
    reads: Option<BufReader<PipeReader>>,
    /// The server's output, `None` once it has ended.
    writes: Option<PipeWriter>,
    errors: Option<PipeWriter>,
    exit: Sender<Option<i32>>,
    /// Gets a message each time the client kills the process.
    pub(crate) killed: Receiver<()>,
}

struct FakeProcess {
    exit: Receiver<Option<i32>>,
    killed: Sender<()>,
}

impl Process for FakeProcess {
    fn wait(&mut self) -> Option<i32> {
        self.exit.recv().ok().flatten()
    }

    fn kill(&mut self) {
        let _unheard = self.killed.send(());
    }
}

/// A client talking to a fake server that hasn't been initialized.
pub(crate) fn connect() -> (Starting, Receiver<Event>, Fake) {
    let (client_reads, server_writes) = pipe().unwrap();
    let (server_reads, client_writes) = pipe().unwrap();
    let (errors_read, errors_written) = pipe().unwrap();
    let (exit, exit_code) = mpsc::channel();
    let (killing, killed) = mpsc::channel();
    let (starting, events) = Server::over(Streams {
        input: Box::new(client_writes),
        output: Box::new(client_reads),
        errors: Box::new(errors_read),
        process: Box::new(FakeProcess {
            exit: exit_code,
            killed: killing,
        }),
    });
    let fake = Fake {
        reads: Some(BufReader::new(server_reads)),
        writes: Some(server_writes),
        errors: Some(errors_written),
        exit,
        killed,
    };
    (starting, events, fake)
}

/// The answer to `initialize`, which waits for the server, so it runs on a thread of its own while the
/// test plays the server.
pub(crate) type Initializing = JoinHandle<Result<(Server, Option<ServerInfo>), RequestError>>;

pub(crate) fn initialize(starting: Starting, init: Init, patience: Duration) -> Initializing {
    thread::spawn(move || starting.initialize(init, patience))
}

/// A client that has initialized a fake server with `settings`.
pub(crate) fn initialized(settings: Value) -> (Server, Receiver<Event>, Fake) {
    let (starting, events, mut fake) = connect();
    let init = Init {
        root: uri("file:///work/lookup"),
        options: json!({}),
        settings,
        experimental: json!({}),
    };
    let initializing = initialize(starting, init, PATIENCE);
    let asked = fake.expect("initialize");
    fake.reply(&asked, json!({"capabilities": {}}));
    let (server, info) = initializing.join().unwrap().unwrap();
    assert_eq!(info, None);
    let _initialized = fake.expect("initialized");
    (server, events, fake)
}

pub(crate) fn uri(text: &str) -> Uri {
    Uri::try_from(text.to_owned()).unwrap()
}

impl Fake {
    /// The next message the client wrote.
    pub(crate) fn next(&mut self) -> Value {
        let body = framing::read(self.reads.as_mut().unwrap())
            .unwrap()
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// Whether the client has closed the server's input with nothing more written to it.
    pub(crate) fn ended(&mut self) -> bool {
        framing::read(self.reads.as_mut().unwrap())
            .unwrap()
            .is_none()
    }

    /// The next message, which must be `method`.
    pub(crate) fn expect(&mut self, method: &str) -> Value {
        let message = self.next();
        assert_eq!(message["method"], method, "{message}");
        message
    }

    pub(crate) fn send(&mut self, message: &Value) {
        framing::write(
            self.writes.as_mut().unwrap(),
            message.to_string().as_bytes(),
        )
        .unwrap();
    }

    /// Writes bytes as they are, framing and all.
    pub(crate) fn send_raw(&mut self, bytes: &[u8]) {
        self.writes.as_mut().unwrap().write_all(bytes).unwrap();
    }

    pub(crate) fn reply(&mut self, request: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    pub(crate) fn write_errors(&mut self, text: &str) {
        self.errors
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    /// Stops reading, as a server whose input has closed.
    pub(crate) fn stop_reading(&mut self) {
        self.reads = None;
    }

    /// Lets a wait for the process end with `code`, leaving the pipes as they are.
    pub(crate) fn reap(&self, code: Option<i32>) {
        self.exit.send(code).unwrap();
    }

    /// Ends the process with `code`, closing its pipes.
    pub(crate) fn exit(&mut self, code: Option<i32>) {
        self.reap(code);
        self.reads = None;
        self.writes = None;
        self.errors = None;
    }
}
