//! A fake language server for the client's tests. The test's own thread plays the server, over in-memory
//! pipes, so each test decides what the server reads and when it answers. Other crates' tests reach it
//! through the `testing` feature, which only their `[dev-dependencies]` turn on, to play a server of their
//! own over [`pipes`].

#![expect(
    clippy::unwrap_used,
    reason = "a failed step of a fake server ends the test"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "a field a message lacks reads as null, which the failed assertion shows"
)]

use std::io::{self, BufRead, BufReader, PipeReader, PipeWriter, Write, pipe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{Event, Fallback, Init, Process, RequestError, Server, Starting, Streams};
use crate::framing;
use crate::types::{Position, Range, ServerInfo, Uri};

/// Longer than any of these tests should wait for the client.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// The server's ends of the pipes a client talks to it over, and its process's controls.
pub struct Ends {
    /// What the client writes.
    pub reads: BufReader<PipeReader>,
    /// The server's output.
    pub writes: PipeWriter,
    /// The server's error output.
    pub errors: PipeWriter,
    /// Ends the client's wait for the process, with the exit code sent.
    pub exit: Sender<Option<i32>>,
    /// Gets a message each time the client kills the process.
    pub killed: Receiver<()>,
}

/// A client talking over in-memory pipes to a server that hasn't been initialized, which copies the
/// server's error output to `log`, and the server's ends of the pipes.
pub fn pipes(log: Option<Box<dyn Write + Send>>) -> (Starting, Receiver<Event>, Ends) {
    let (client_reads, server_writes) = pipe().unwrap();
    let (server_reads, client_writes) = pipe().unwrap();
    let (errors_read, errors_written) = pipe().unwrap();
    let (exit, exit_code) = mpsc::channel();
    let (killing, killed) = mpsc::channel();
    let (starting, events) = Server::over(Streams {
        input: Box::new(client_writes),
        output: Box::new(client_reads),
        errors: Box::new(errors_read),
        log,
        process: Box::new(FakeProcess {
            exit: exit_code,
            killed: killing,
        }),
    });
    let ends = Ends {
        reads: BufReader::new(server_reads),
        writes: server_writes,
        errors: errors_written,
        exit,
        killed,
    };
    (starting, events, ends)
}

/// The next message on `input`, or `None` once it has ended between messages. A message cut off, framed
/// wrong or not JSON fails.
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let body = framing::read(input)?;
    body.map(|body| serde_json::from_slice(&body).map_err(io::Error::other))
        .transpose()
}

/// Writes `message` to `output`, framed.
pub fn write_message(output: &mut impl Write, message: &Value) -> io::Result<()> {
    framing::write(output, message.to_string().as_bytes())
}

/// The server's side of the pipes, and its process's controls.
pub struct Fake {
    /// What the client writes. `None` once the server has stopped reading.
    reads: Option<BufReader<PipeReader>>,
    /// The server's output, `None` once it has ended.
    writes: Option<PipeWriter>,
    errors: Option<PipeWriter>,
    exit: Sender<Option<i32>>,
    /// Gets a message each time the client kills the process.
    pub killed: Receiver<()>,
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
pub fn connect() -> (Starting, Receiver<Event>, Fake) {
    connect_logged(None)
}

/// A client talking to a fake server that hasn't been initialized, which copies the server's error output
/// to `log`.
pub fn connect_logged(log: Option<Box<dyn Write + Send>>) -> (Starting, Receiver<Event>, Fake) {
    let (starting, events, ends) = pipes(log);
    let fake = Fake {
        reads: Some(ends.reads),
        writes: Some(ends.writes),
        errors: Some(ends.errors),
        exit: ends.exit,
        killed: ends.killed,
    };
    (starting, events, fake)
}

/// The answer to `initialize`, which waits for the server, so it runs on a thread of its own while the
/// test plays the server.
pub type Initializing = JoinHandle<Result<(Server, Option<ServerInfo>), RequestError>>;

pub fn initialize(starting: Starting, init: Init, patience: Duration) -> Initializing {
    thread::spawn(move || starting.initialize(init, patience))
}

/// A client that has initialized a fake server with `settings`.
pub fn initialized(settings: Value) -> (Server, Receiver<Event>, Fake) {
    initialized_with(settings, None, json!({}))
}

/// A client that has initialized a fake server with `settings`, and `fallback` if it refuses them. The
/// server answers `initialize` with `capabilities`.
pub fn initialized_with(
    settings: Value,
    fallback: Option<Fallback>,
    capabilities: Value,
) -> (Server, Receiver<Event>, Fake) {
    let (starting, events, mut fake) = connect();
    let init = Init {
        root: uri("file:///work/lookup"),
        options: json!({}),
        settings,
        experimental: json!({}),
        fallback,
    };
    let initializing = initialize(starting, init, PATIENCE);
    let asked = fake.expect("initialize");
    fake.reply(&asked, json!({"capabilities": capabilities}));
    let (server, info) = initializing.join().unwrap().unwrap();
    assert_eq!(info, None);
    let _initialized = fake.expect("initialized");
    (server, events, fake)
}

pub fn uri(text: &str) -> Uri {
    Uri::try_from(text.to_owned()).unwrap()
}

/// The characters `from` up to `to` of line `line`.
pub fn range(line: u32, from: u32, to: u32) -> Range {
    Range {
        start: Position {
            line,
            character: from,
        },
        end: Position {
            line,
            character: to,
        },
    }
}

impl Fake {
    /// The next message the client wrote.
    #[expect(
        clippy::should_implement_trait,
        reason = "the messages never end, so the fake is no iterator"
    )]
    pub fn next(&mut self) -> Value {
        read_message(self.reads.as_mut().unwrap()).unwrap().unwrap()
    }

    /// Whether the client has closed the server's input with nothing more written to it.
    pub fn ended(&mut self) -> bool {
        read_message(self.reads.as_mut().unwrap())
            .unwrap()
            .is_none()
    }

    /// The next message, which must be `method`.
    pub fn expect(&mut self, method: &str) -> Value {
        let message = self.next();
        assert_eq!(message["method"], method, "{message}");
        message
    }

    pub fn send(&mut self, message: &Value) {
        write_message(self.writes.as_mut().unwrap(), message).unwrap();
    }

    /// Writes bytes as they are, framing and all.
    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.writes.as_mut().unwrap().write_all(bytes).unwrap();
    }

    pub fn reply(&mut self, request: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    pub fn write_errors(&mut self, text: &str) {
        self.errors
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    /// Stops reading, as a server whose input has closed.
    pub fn stop_reading(&mut self) {
        self.reads = None;
    }

    /// Lets a wait for the process end with `code`, leaving the pipes as they are.
    pub fn reap(&self, code: Option<i32>) {
        self.exit.send(code).unwrap();
    }

    /// Ends the process with `code`, closing its pipes.
    pub fn exit(&mut self, code: Option<i32>) {
        self.reap(code);
        self.reads = None;
        self.writes = None;
        self.errors = None;
    }
}
