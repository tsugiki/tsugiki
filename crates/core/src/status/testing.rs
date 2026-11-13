use super::*;
use std::sync::mpsc::{self, Receiver};

/// A board that passes on changes to stored facts and drops status changes.
pub(crate) fn changes_board() -> (StatusBoard, Receiver<Change>) {
    let (passed, seen) = mpsc::channel();
    let board =
        StatusBoard::new(Box::new(|_| {})).with_changes(Box::new(move |change: &Change| {
            passed.send(change.clone()).unwrap();
        }));
    (board, seen)
}

/// A board that passes on the reads of items on screen that failed, and drops status changes.
pub(crate) fn failed_reads_board() -> (StatusBoard, Receiver<FailedRead>) {
    let (passed, seen) = mpsc::channel();
    let board = StatusBoard::new(Box::new(|_| {})).with_failed_reads(Box::new(
        move |failed: &FailedRead| {
            passed.send(failed.clone()).unwrap();
        },
    ));
    (board, seen)
}

/// A board that passes on the work for the mirrors it is handed, with the Repository it is for.
pub(crate) fn heads_board() -> (StatusBoard, Receiver<(RepositoryName, MirrorWork)>) {
    let (passed, seen) = mpsc::channel();
    let board = StatusBoard::new(Box::new(|_| {})).with_heads(Box::new(
        move |repository: &RepositoryName, work: MirrorWork| {
            passed.send((repository.clone(), work)).unwrap();
        },
    ));
    (board, seen)
}

pub(crate) fn quiet_board() -> StatusBoard {
    StatusBoard::new(Box::new(|_| {}))
}

/// A board, and where it sends what it announces, so a test can also wait for a status from another
/// thread.
pub(crate) fn watched_board() -> (StatusBoard, Receiver<Status>) {
    let (passed, seen) = mpsc::channel();
    let board = StatusBoard::new(Box::new(move |status: &Status| {
        passed.send(status.clone()).unwrap();
    }));
    (board, seen)
}

/// What the board has announced since the last time a test looked.
pub(crate) fn announced(seen: &Receiver<Status>) -> Vec<Status> {
    seen.try_iter().collect()
}

/// The last thing the board announced, which is what the app is showing.
pub(crate) fn showing(seen: &Receiver<Status>) -> Status {
    seen.try_iter().last().unwrap()
}
