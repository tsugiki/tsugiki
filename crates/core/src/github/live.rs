//! What touches the outside world: the real `git`, and GitHub through `tsugiki-token`, the crate that holds
//! the network and `gh` (ADR 0019). Tests replace the network (ADR 0009), so this file is left out of the
//! coverage gate. Tests run the real `git` through it.

#![expect(
    clippy::disallowed_types,
    reason = "this is the one file in the core that clippy.toml lets run git"
)]

use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;
use std::process::{ChildStderr, Command, ExitStatus, Stdio};
use std::thread;
#[cfg(not(windows))]
use std::time::{Duration, Instant};

use process_wrap::std::ChildWrapper;
use tsugiki_token::Https;
#[cfg(windows)]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable, https};
use crate::token::Token;

/// GitHub through the token crate's client. `https.rs` builds what it sends and reads what comes back.
impl Transport for Https {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        https::reply(self.post_graphql(token, https::graphql_body(request)))
    }

    fn get(
        &self,
        token: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        https::conditional_reply(self.get_rest(token, request.path, request.etag))
    }
}

/// Starts `git` with `args` and `input` on its standard input, its error output piped for its progress, which
/// it writes in English.
///
/// A Repository git can't read fails rather than asking for a password nobody sees: no terminal prompt, and
/// no credential helper, since Git Credential Manager on Windows would open a sign-in window.
pub(crate) fn start_git(args: &[&OsStr], input: &[u8]) -> io::Result<(StartedGit, ChildStderr)> {
    let mut command = tsugiki_lsp::set_up("git", |command| {
        command
            .args(["-c", "credential.helper="])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
    });
    // git hands a download to helpers (`git-remote-https`, `index-pack`), and the `git` on PATH on Windows
    // is a launcher that starts the real one. Each helper joins git's job object, or its process group, as
    // it starts, so `StartedGit::stop` ends them all. Reading the tree when stopping misses a helper that
    // starts while it reads, and that helper goes on holding files in the mirror (#208).
    let mut git = StartedGit(tsugiki_lsp::windowless(&mut command).spawn()?);
    // On a thread of its own, so a git that writes before it reads can't leave both waiting on a full pipe.
    // A git that stops reading says why on its error output. Closing the input tells git that is all.
    if let Some(mut stdin) = git.0.stdin().take() {
        let input = input.to_vec();
        #[expect(
            unused_results,
            reason = "the write ends once git has read the input or gone, and git says why it stopped reading"
        )]
        thread::spawn(move || drop(stdin.write_all(&input)));
    }
    match git.0.stderr().take() {
        Some(progress) => Ok((git, progress)),
        None => {
            git.stop();
            Err(io::Error::other("git's error output wasn't piped"))
        }
    }
}

/// A git that `start_git` started, with the helpers it starts in turn.
pub(crate) struct StartedGit(Box<dyn ChildWrapper>);

impl StartedGit {
    /// Waits for git to end, and says if it did what it was asked.
    pub(crate) fn succeeded(&mut self) -> bool {
        self.0.wait().is_ok_and(|status| status.success())
    }

    /// Ends git and its helpers, so none goes on downloading after Tsugiki quits, and waits for git. A helper
    /// can take a moment longer to end, and git's error output closes once the last one has. Off Windows,
    /// killing the helpers that started meanwhile can add up to `STRAGGLERS_PATIENCE`. A failure leaves a
    /// trace.
    pub(crate) fn stop(&mut self) {
        if let Err(cause) = self.end() {
            unwaited(&cause);
        }
    }

    /// Stops git as [`StartedGit::stop`] does, and says if git had ended before the kill, having done what
    /// it was asked.
    pub(crate) fn stop_finished(&mut self) -> bool {
        (self.end())
            .inspect_err(unwaited)
            .is_ok_and(|status| status.success())
    }

    /// Ends git and its helpers, as [`StartedGit::stop`] says, and gives how git ended.
    fn end(&mut self) -> io::Result<ExitStatus> {
        if let Err(cause) = self.0.start_kill() {
            log::warn!("Tsugiki can't stop git's helpers: {cause}");
            // git at least, so the wait doesn't last until git ends on its own.
            if let Err(cause) = self.0.inner_mut().start_kill() {
                log::warn!("Tsugiki can't stop git: {cause}");
            }
        }
        let ended = self.0.wait();
        #[cfg(not(windows))]
        self.end_stragglers();
        ended
    }

    /// Kills git's process group again until nobody is left in it. macOS kills the processes it finds in
    /// the group when the kill starts, so a process that forks meanwhile leaves a child the kill missed
    /// (#309), and that child can fork in turn. A job object on Windows lets no process escape that way.
    ///
    /// The group outlives git, whose number it bears, while a helper is left in it. The loop ends at the
    /// first kill that finds the group empty, so another group can take that number only in the moment
    /// between two kills.
    #[cfg(not(windows))]
    fn end_stragglers(&self) {
        let deadline = Instant::now() + STRAGGLERS_PATIENCE;
        loop {
            match self.0.signal(libc::SIGKILL) {
                Ok(()) => {}
                Err(cause) if cause.raw_os_error() == Some(libc::ESRCH) => return,
                Err(cause) => {
                    log::warn!("Tsugiki can't stop git's helpers: {cause}");
                    return;
                }
            }
            if Instant::now() > deadline {
                log::warn!("Tsugiki can't stop git's helpers: some are still running");
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Logs that Tsugiki couldn't wait for git to end, for `cause`.
fn unwaited(cause: &io::Error) {
    log::warn!("Tsugiki can't wait for git: {cause}");
}

/// The time `StartedGit::stop` spends killing helpers that started while it killed the others. Short beside
/// the second quitting gives, since the clone a stop finishes is marked only after it.
#[cfg(not(windows))]
const STRAGGLERS_PATIENCE: Duration = Duration::from_millis(200);

/// Runs a `git` that only reads local files, with `args`, and gives its output. Its last line of error output
/// says why it failed.
pub(crate) fn read_git(args: &[&OsStr]) -> io::Result<String> {
    let output = local_git(&mut Command::new("git"), args)
        .stdin(Stdio::null())
        .output()?;
    git_output(&output).map(|stdout| String::from_utf8_lossy(stdout).into_owned())
}

/// Runs a `git` that only reads local files, with `args` and `input` on its standard input, and gives its
/// output as git wrote it. Its last line of error output says why it failed.
pub(crate) fn read_git_bytes(args: &[&OsStr], input: Vec<u8>) -> io::Result<Vec<u8>> {
    git_bytes(local_git(&mut Command::new("git"), args), input)
}

/// The same, with the environment variables `envs` set for git.
pub(crate) fn read_git_bytes_with_env(
    args: &[&OsStr],
    input: Vec<u8>,
    envs: &[(&str, &OsStr)],
) -> io::Result<Vec<u8>> {
    git_bytes(
        local_git(&mut Command::new("git"), args).envs(envs.iter().copied()),
        input,
    )
}

/// The blobs `names` names, read from the git folder `folder` with one `git cat-file --batch`: the bytes of
/// each, or nothing for a name git has no blob for. A name is a blob's id or `<commit>:<path>`, and a PR's
/// author picks its paths. git reads a name a line and drops a carriage return at a line's end, so a name
/// that holds a line feed, or ends in a carriage return, would read as another name and send each blob after
/// it to the wrong place. Such a name stays out of git's input and gets nothing. Output cut short ends the
/// list.
pub(crate) fn read_blobs(folder: &Path, names: &[&str]) -> io::Result<Vec<Option<Vec<u8>>>> {
    let readable = |name: &str| !name.contains('\n') && !name.ends_with('\r');
    // At debug: a PR's author picks the path, the user can't change it, and each read of the file says it.
    for name in names.iter().filter(|name| !readable(name)) {
        log::debug!("Tsugiki can't read {name:?} from git: its name has a line break");
    }
    let input: Vec<u8> = (names.iter())
        .filter(|name| readable(name))
        .flat_map(|name| name.bytes().chain(*b"\n"))
        .collect();
    if input.is_empty() {
        return Ok(names.iter().map(|_| None).collect());
    }
    let args = [
        OsStr::new("-C"),
        folder.as_os_str(),
        OsStr::new("cat-file"),
        OsStr::new("--batch"),
    ];
    let mut blobs = parse_batch(&read_git_bytes(&args, input)?).into_iter();
    Ok((names.iter())
        .map_while(|name| {
            if readable(name) {
                blobs.next()
            } else {
                Some(None)
            }
        })
        .collect())
}

/// What `git cat-file --batch` printed: for each name, `<id> <type> <size>`, a line feed, the object and a
/// line feed, or a line such as `<name> missing`. Only a blob gives bytes. Output cut short ends the list.
fn parse_batch(mut output: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut objects = Vec::new();
    while let Some(end) = output.iter().position(|&byte| byte == b'\n') {
        let header = String::from_utf8_lossy(output.get(..end).unwrap_or_default());
        output = output.get(end.saturating_add(1)..).unwrap_or_default();
        let found = match header.split(' ').collect::<Vec<_>>().as_slice() {
            [_, kind, size] => size
                .parse::<usize>()
                .ok()
                .map(|size| (*kind == "blob", size)),
            _ => None,
        };
        let Some((blob, size)) = found else {
            log::warn!("Tsugiki can't read a file from git: git says {header}");
            objects.push(None);
            continue;
        };
        let Some(object) = output.get(..size) else {
            break;
        };
        objects.push(blob.then(|| object.to_vec()));
        output = output.get(size.saturating_add(1)..).unwrap_or_default();
    }
    objects
}

fn git_bytes(command: &mut Command, input: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut git = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // On a thread of its own, so a git that writes before it has read everything can't leave both waiting on
    // a full pipe. Closing the input tells git that is all.
    if let Some(mut stdin) = git.stdin.take() {
        #[expect(
            unused_results,
            reason = "the write ends once git has read the input or gone, and git says why it failed"
        )]
        thread::spawn(move || drop(stdin.write_all(&input)));
    }
    let output = git.wait_with_output()?;
    git_output(&output).map(<[u8]>::to_vec)
}

/// `command`, a `git`, set to run with `args`, only reading local files, and writing in English with no
/// console window.
fn local_git<'c>(command: &'c mut Command, args: &[&OsStr]) -> &'c mut Command {
    without_window(command.args(args).env("LC_ALL", "C").env("LANGUAGE", "C"))
}

/// The output of a `git`, or the gist of its error output if it failed.
fn git_output(output: &std::process::Output) -> io::Result<&[u8]> {
    if output.status.success() {
        Ok(&output.stdout)
    } else {
        let errors = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(gist(&errors).to_owned()))
    }
}

/// git's last `fatal:` or `error:` line, or else its last line. Hints follow the reason, as in
/// `fatal: detected dubious ownership`, whose last line is the command that adds a `safe.directory`.
fn gist(errors: &str) -> &str {
    errors
        .lines()
        .rev()
        .find(|line| line.starts_with("fatal: ") || line.starts_with("error: "))
        .or_else(|| errors.lines().last())
        .unwrap_or("")
}

/// Keeps a console program from opening a window on Windows, where Tsugiki itself has no console.
#[cfg(windows)]
fn without_window(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW.0)
}

/// Only Windows opens a window for a console program.
#[cfg(not(windows))]
fn without_window(command: &mut Command) -> &mut Command {
    command
}

#[cfg(test)]
mod tests;
