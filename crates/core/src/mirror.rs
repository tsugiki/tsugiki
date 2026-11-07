//! Each Repository's **Mirror**: a bare clone in its folder (ADR 0002, ADR 0010). One worker thread writes
//! to the mirrors, beside the session's sync queue, and clones them one at a time.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::folders::Folders;
use crate::github::live;
use crate::repository::RepositoryName;

/// How far a clone has got, from what `git clone --progress` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum CloneProgress {
    /// GitHub is still counting and packing what it sends.
    Starting,
    /// The objects coming down.
    Receiving { percent: u8 },
    /// Indexing what came down, which takes about as long again.
    Resolving { percent: u8 },
}

/// The clone under way, for the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Cloning {
    pub repository: RepositoryName,
    pub progress: CloneProgress,
}

/// Where git clones to, in the Repository's folder, so a clone cut off never looks like a mirror.
const PARTIAL: &str = "mirror.git.partial";

/// Says the clone under way each time it moves on, and `None` when it ends.
type AnnounceCloning = Box<dyn FnMut(Option<&Cloning>) + Send>;

/// Where clones come from: GitHub, or in tests a folder of bare repositories laid out as GitHub's are.
pub struct Remotes {
    base: String,
}

impl Remotes {
    pub fn github() -> Self {
        Self::under("https://github.com/".to_owned())
    }

    /// Remotes at `base` followed by `owner/name.git`.
    pub fn under(base: String) -> Self {
        Self { base }
    }

    fn url(&self, repository: &RepositoryName) -> String {
        format!(
            "{}{}/{}.git",
            self.base,
            repository.owner(),
            repository.name()
        )
    }
}

enum Message {
    Clone(RepositoryName, Turn),
    Progress(CloneProgress),
    /// git closed its error output, so it has ended or is about to. Its last line that wasn't progress says
    /// why a clone failed.
    Ended {
        last_words: String,
    },
    Stop,
}

/// Where a Repository waits for its clone.
#[derive(Debug, Clone, Copy)]
enum Turn {
    Next,
    Last,
}

/// The mirrors' one writer.
pub struct Mirrors {
    messages: Sender<Message>,
    finished: Receiver<()>,
}

/// A way to ask for mirrors. It returns at once.
#[derive(Clone)]
pub struct MirrorRequests {
    messages: Sender<Message>,
}

impl MirrorRequests {
    /// Clones `repository`'s mirror after the ones already waiting, unless it has one.
    pub fn clone_mirror(&self, repository: RepositoryName) {
        self.send(Message::Clone(repository, Turn::Last));
    }

    /// Clones `repository`'s mirror before the ones waiting, as the open Repository's goes first. A clone
    /// under way finishes first.
    pub fn clone_mirror_next(&self, repository: RepositoryName) {
        self.send(Message::Clone(repository, Turn::Next));
    }

    fn send(&self, message: Message) {
        if self.messages.send(message).is_err() {
            log::warn!("Tsugiki has stopped cloning mirrors, so it can't clone another");
        }
    }
}

impl Mirrors {
    /// Starts the worker. Without `folders` it has nowhere to put a mirror, and logs each request instead.
    pub fn start(
        folders: Result<Folders, Error>,
        remotes: Remotes,
        announce: AnnounceCloning,
    ) -> Self {
        let (messages, incoming) = mpsc::channel();
        let (finishing, finished) = mpsc::channel::<()>();
        let mut worker = Worker {
            folders,
            remotes,
            announce,
            messages: messages.clone(),
            waiting: VecDeque::new(),
            clone: None,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the worker is over when this goes, however the thread ends.
            let _finishing = finishing;
            worker.run(&incoming);
        });
        Self { messages, finished }
    }

    pub fn requests(&self) -> MirrorRequests {
        MirrorRequests {
            messages: self.messages.clone(),
        }
    }

    /// Stops a clone under way, with the helpers git started, and gives the worker up to `patience` to end.
    /// The next launch starts that clone again from an empty folder.
    pub fn stop(&self, patience: Duration) {
        self.begin_stopping();
        self.wait_for_end(patience);
    }

    /// Tells the worker to stop, and returns at once, so the caller can stop other threads meanwhile.
    pub fn begin_stopping(&self) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the worker has already finished, which is what this asks for"
        )]
        let _ = self.messages.send(Message::Stop);
    }

    /// Waits up to `patience` for the worker to end, after `begin_stopping`.
    pub fn wait_for_end(&self, patience: Duration) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "ending git takes a moment, so only a stuck disk outlasts `patience`, and quitting goes on"
        )]
        let _ = self.finished.recv_timeout(patience);
    }
}

/// The worker holds a sender for git's progress, so its messages never run out: without this, dropping
/// `Mirrors` would leave it, and a clone, running.
impl Drop for Mirrors {
    fn drop(&mut self) {
        self.begin_stopping();
    }
}

/// A clone under way.
struct Clone {
    cloning: Cloning,
    git: Child,
    partial: PathBuf,
    mirror: PathBuf,
}

struct Worker {
    folders: Result<Folders, Error>,
    remotes: Remotes,
    announce: AnnounceCloning,
    /// For the thread that reads each git's progress.
    messages: Sender<Message>,
    waiting: VecDeque<RepositoryName>,
    clone: Option<Clone>,
}

impl Worker {
    fn run(&mut self, incoming: &Receiver<Message>) {
        loop {
            self.start_next();
            match incoming.recv() {
                Ok(Message::Clone(repository, turn)) => self.wait(repository, turn),
                Ok(Message::Progress(progress)) => self.moved_on(progress),
                Ok(Message::Ended { last_words }) => {
                    // Each git's end comes while its clone is the one under way.
                    let ended = self.clone.take();
                    ended
                        .into_iter()
                        .for_each(|clone| self.finish(clone, &last_words));
                }
                Ok(Message::Stop) | Err(_) => break,
            }
        }
        if let Some(mut clone) = self.clone.take() {
            live::stop_git(&mut clone.git);
        }
    }

    fn wait(&mut self, repository: RepositoryName, turn: Turn) {
        let cloning = self.clone.as_ref().map(|clone| &clone.cloning.repository);
        if cloning.is_some_and(|cloning| cloning.is_same(&repository)) {
            return;
        }
        let waiting = self.waiting.iter().position(|one| one.is_same(&repository));
        match (turn, waiting) {
            (Turn::Last, Some(_)) => {}
            (Turn::Last, None) => self.waiting.push_back(repository),
            (Turn::Next, waiting) => {
                if let Some(index) = waiting {
                    drop(self.waiting.remove(index));
                }
                self.waiting.push_front(repository);
            }
        }
    }

    fn start_next(&mut self) {
        while self.clone.is_none() {
            let Some(repository) = self.waiting.pop_front() else {
                return;
            };
            let folders = match &self.folders {
                Ok(folders) => folders,
                Err(problem) => {
                    log::warn!(
                        "Tsugiki can't clone {repository}'s mirror: {}",
                        problem.message
                    );
                    continue;
                }
            };
            let folder = folders.repository(&repository);
            if folders.mirror(&repository).exists() {
                continue;
            }
            match self.start_clone(repository, folder) {
                Ok(clone) => {
                    (self.announce)(Some(&clone.cloning));
                    self.clone = Some(clone);
                }
                Err((repository, cause)) => {
                    log::warn!("Tsugiki can't start cloning {repository}'s mirror: {cause}");
                }
            }
        }
    }

    fn start_clone(
        &self,
        repository: RepositoryName,
        folder: PathBuf,
    ) -> Result<Clone, (RepositoryName, io::Error)> {
        let partial = folder.join(PARTIAL);
        let mirror = folder.join("mirror.git");
        let git = start_git(&self.remotes.url(&repository), &folder, &partial);
        match git {
            Ok((git, progress)) => {
                let messages = self.messages.clone();
                let _ = thread::spawn(move || read_progress_until_done(progress, &messages));
                Ok(Clone {
                    cloning: Cloning {
                        repository,
                        progress: CloneProgress::Starting,
                    },
                    git,
                    partial,
                    mirror,
                })
            }
            Err(cause) => Err((repository, cause)),
        }
    }

    fn moved_on(&mut self, progress: CloneProgress) {
        if let Some(clone) = &mut self.clone
            && clone.cloning.progress != progress
        {
            clone.cloning.progress = progress;
            (self.announce)(Some(&clone.cloning));
        }
    }

    /// Moves a finished clone to where a mirror goes, or deletes what a failed one left. git's last words
    /// say why it failed.
    fn finish(&mut self, mut clone: Clone, last_words: &str) {
        let repository = &clone.cloning.repository;
        let cloned = clone.git.wait().is_ok_and(|status| status.success());
        let ended = if cloned {
            fs::rename(&clone.partial, &clone.mirror)
        } else {
            Err(io::Error::other(last_words.to_owned()))
        };
        match ended {
            Ok(()) => log::info!("Tsugiki cloned {repository}'s mirror"),
            Err(why) => {
                log::warn!("Tsugiki couldn't clone {repository}'s mirror: {why}");
                // What is left goes before the next clone of this Repository, which says if it can't.
                drop(fs::remove_dir_all(&clone.partial));
            }
        }
        (self.announce)(None);
    }
}

/// Starts `git clone` into `partial`, in the Repository's `folder`, deleting what a clone cut off by quitting
/// left there.
fn start_git(url: &str, folder: &Path, partial: &Path) -> io::Result<(Child, ChildStderr)> {
    if partial.exists() {
        fs::remove_dir_all(partial)?;
    }
    fs::create_dir_all(folder)?;
    let args = ["clone", "--bare", "--no-tags", "--progress", url].map(OsStr::new);
    live::start_git(&[args.as_slice(), &[partial.as_os_str()]].concat())
}

/// Passes on each line of progress git writes, until git closes its error output. git ends a line of
/// progress with a carriage return, and other lines with a newline.
fn read_progress_until_done(progress: ChildStderr, messages: &Sender<Message>) {
    let mut last_words = String::new();
    let mut line = Vec::new();
    for byte in BufReader::new(progress).bytes().map_while(Result::ok) {
        if byte != b'\r' && byte != b'\n' {
            line.push(byte);
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        match read_progress(&text) {
            Some(progress) => drop(messages.send(Message::Progress(progress))),
            None if !text.trim().is_empty() => last_words = text.trim().to_owned(),
            None => {}
        }
        line.clear();
    }
    drop(messages.send(Message::Ended { last_words }));
}

/// Reads one line of `git clone --progress`. Lines from the server, and lines without a percentage, say
/// nothing about the clone here.
fn read_progress(line: &str) -> Option<CloneProgress> {
    let (stage, rest) = line.split_once(':')?;
    let (number, _) = rest.trim_start().split_once('%')?;
    let percent = number.parse().ok().filter(|percent| *percent <= 100)?;
    match stage {
        "Receiving objects" => Some(CloneProgress::Receiving { percent }),
        "Resolving deltas" => Some(CloneProgress::Resolving { percent }),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
