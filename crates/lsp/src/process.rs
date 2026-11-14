//! Starts a language server's process. This thin layer is the one part of the crate tests don't run, so it
//! is left out of the coverage gate: the tests talk to a fake server over in-memory pipes instead.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file in the LSP client that starts a process"
)]

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;

use process_wrap::std::{ChildWrapper, CommandWrap};
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
    /// Starts the server `launch` names, with no window of its own on Windows. The processes it starts in
    /// turn end with it.
    pub fn start(launch: &Launch) -> io::Result<(Starting, Receiver<Event>)> {
        let mut command = Command::new(&launch.program);
        let _ = command
            .args(&launch.args)
            .envs(launch.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&launch.folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut command = CommandWrap::from(command);
        #[cfg(windows)]
        let _ = command
            .wrap(process_wrap::std::CreationFlags(CREATE_NO_WINDOW))
            .wrap(process_wrap::std::JobObject);
        #[cfg(not(windows))]
        let _ = command.wrap(process_wrap::std::ProcessGroup::leader());
        let mut child = command.spawn()?;
        let input = child.stdin().take();
        let output = child.stdout().take();
        let errors = child.stderr().take();
        let mut process = Child(child);
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
            process: Box::new(process),
        }))
    }
}

/// A server's process, with the processes it started.
struct Child(Box<dyn ChildWrapper>);

impl Process for Child {
    /// Waits for the server alone, then ends what it left running. A wait on the whole job object, or
    /// process group, would last as long as a process the server started, which could be never.
    fn wait(&mut self) -> Option<i32> {
        let waited = self.0.inner_mut().wait();
        if let Err(cause) = self.0.start_kill() {
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
        if let Err(cause) = self.0.start_kill() {
            log::warn!("Tsugiki can't stop a language server: {cause}");
        }
    }
}
