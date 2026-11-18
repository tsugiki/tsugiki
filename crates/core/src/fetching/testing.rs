use std::sync::mpsc::{self, Receiver};

use super::{Fetching, SessionMessage};
use crate::mirror::HeardRequests;

/// A handle, what reaches the session through it, and what reaches the Mirror worker.
pub(crate) fn handle() -> (Fetching, Receiver<SessionMessage>, HeardRequests) {
    let (messages, incoming) = mpsc::channel();
    let (mirrors, heard) = HeardRequests::new();
    (Fetching::new(messages, mirrors), incoming, heard)
}
