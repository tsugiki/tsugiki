//! Starts a language server's process, or runs it once with `--version`. This thin layer is the one part of
//! the crate tests don't run, so it is left out of the coverage gate: the tests talk to a fake server over
//! in-memory pipes instead, and the core's tests run `--version` on real programs.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file in the LSP client that starts a process"
)]

use std::ffi::OsString;
use std::io::{self, Read, Write};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use process_wrap::std::CommandWrapper;
use process_wrap::std::{ChildWrapper, CommandWrap};
#[cfg(windows)]
use win32job::{ExtendedLimitInfo, Job};
#[cfg(windows)]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::client::{Event, Process, Server, Starting, Streams};

/// A server's program, its arguments, the folder it starts in and the variables it gets.
#[derive(Clone, Debug)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The folder the server starts in.
    pub folder: PathBuf,
    /// Variables the server gets on top of Tsugiki's own, which the processes it starts inherit. An empty
    /// value overrides one Tsugiki has.
    pub env: Vec<(OsString, OsString)>,
}

impl Server {
    /// Starts the server `launch` names, with no window of its own on Windows, and copies its error output
    /// to `log` if there is one. The processes it starts in turn end with it, and on Windows the server and
    /// those processes end with Tsugiki's process.
    pub fn start(
        launch: &Launch,
        log: Option<Box<dyn Write + Send>>,
    ) -> io::Result<(Starting, Receiver<Event>)> {
        let mut command = Command::new(&launch.program);
        let _ = command
            .args(&launch.args)
            .envs(launch.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&launch.folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut command = windowless(CommandWrap::from(command));
        #[cfg(windows)]
        let job = EndsWithTsugiki::create();
        #[cfg(windows)]
        if let Some(job) = &job {
            let _ = command.wrap(EndsWithTsugiki(Arc::clone(&job.0)));
        }
        let mut child = command.spawn()?;
        let input = child.stdin().take();
        let output = child.stdout().take();
        let errors = child.stderr().take();
        let mut process = Child {
            process: child,
            #[cfg(windows)]
            _job: job,
        };
        let (Some(input), Some(output), Some(errors)) = (input, output, errors) else {
            process.kill();
            let _code = process.wait();
            return Err(io::Error::other(
                "the language server's streams weren't piped",
            ));
        };
        Ok(Self::over(Streams {
            input: Box::new(input),
            output: Box::new(output),
            errors: Box::new(errors),
            log,
            process: Box::new(process),
        }))
    }
}

/// The shortest wait between two looks at whether a `--version` run has ended.
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// Runs `program --version` with nothing to read and no window of its own on Windows, and gives what it
/// printed: its output, or its error output if it printed nothing else. One still running after `patience`
/// is killed, with the processes it started, and fails.
pub fn version_of(program: &Path, patience: Duration) -> io::Result<String> {
    let mut command = Command::new(program);
    let _ = command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = windowless(CommandWrap::from(command)).spawn()?;
    let output = child.stdout().take().map(read_all);
    let errors = child.stderr().take().map(read_all);
    let until = Instant::now() + patience;
    while child.try_wait()?.is_none() {
        if Instant::now() >= until {
            let _kill = child.start_kill();
            let _code = child.wait();
            let seconds = patience.as_secs();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("it was still running after {seconds} s"),
            ));
        }
        thread::sleep(LOOK_EVERY);
    }
    let text = |reading: Option<JoinHandle<String>>| {
        reading
            .and_then(|reading| reading.join().ok())
            .unwrap_or_default()
    };
    let output = text(output);
    Ok(if output.trim().is_empty() {
        text(errors)
    } else {
        output
    })
}

/// Reads all of `stream` on a thread of its own, so a program that fills one pipe while Tsugiki reads the
/// other can't stop.
fn read_all(mut stream: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _read = stream.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `command` with no console window of its own on Windows, in a job object, or in a process group of its
/// own elsewhere, so the processes it starts end with it.
fn windowless(mut command: CommandWrap) -> CommandWrap {
    #[cfg(windows)]
    let _ = command
        .wrap(process_wrap::std::CreationFlags(CREATE_NO_WINDOW))
        .wrap(process_wrap::std::JobObject);
    #[cfg(not(windows))]
    let _ = command.wrap(process_wrap::std::ProcessGroup::leader());
    command
}

/// A server's process, with the processes it started.
struct Child {
    process: Box<dyn ChildWrapper>,
    /// Kills what is left of the server once the `Child` drops, or Tsugiki's process ends.
    #[cfg(windows)]
    _job: Option<EndsWithTsugiki>,
}

impl Process for Child {
    /// Waits for the server alone, then ends what it left running. A wait on the whole job object, or
    /// process group, would last as long as a process the server started, which could be never.
    fn wait(&mut self) -> Option<i32> {
        let waited = self.process.inner_mut().wait();
        if let Err(cause) = self.process.start_kill() {
            log::debug!("Tsugiki found nothing a language server left running to stop: {cause}");
        }
        match waited {
            Ok(status) => status.code(),
            Err(cause) => {
                log::warn!("Tsugiki can't wait for a language server: {cause}");
                None
            }
        }
    }

    fn kill(&mut self) {
        if let Err(cause) = self.process.start_kill() {
            log::warn!("Tsugiki can't stop a language server: {cause}");
        }
    }
}

/// A job object that kills the processes in it once its last handle closes, as when Tsugiki's process ends,
/// even in a crash. process-wrap's own job object leaves them running then.
#[cfg(windows)]
#[derive(Debug)]
struct EndsWithTsugiki(Arc<Job>);

#[cfg(windows)]
impl EndsWithTsugiki {
    /// A new job, or none if Windows refuses one: the server then runs without it.
    fn create() -> Option<Self> {
        match Job::create_with_limit_info(ExtendedLimitInfo::new().limit_kill_on_job_close()) {
            Ok(job) => Some(Self(Arc::new(job))),
            Err(cause) => {
                log::warn!(
                    "Tsugiki can't make a job object, so a crash may leave a language server running: {cause}"
                );
                None
            }
        }
    }
}

#[cfg(windows)]
impl CommandWrapper for EndsWithTsugiki {
    /// Puts the server in the job while it is still suspended, before process-wrap's job object resumes it,
    /// so each process it starts is in the job too.
    fn post_spawn(
        &mut self,
        _command: &mut Command,
        child: &mut std::process::Child,
        _core: &CommandWrap,
    ) -> io::Result<()> {
        let handle = child.as_raw_handle().addr().cast_signed();
        if let Err(cause) = self.0.assign_process(handle) {
            log::warn!(
                "Tsugiki can't put a language server in its job object, so a crash may leave it running: {cause}"
            );
        }
        Ok(())
    }
}
