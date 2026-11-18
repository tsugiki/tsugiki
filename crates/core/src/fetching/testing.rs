use std::sync::mpsc::{self, Receiver};

use super::{Fetched, Fetching, Item, Marked, SessionMessage};
use crate::latest::{self, LatestWriter};
use crate::mirror::HeardRequests;
use crate::repository_settings::SavedBy;

/// `message` as a test names it: what it reads, refreshes, changes or marks, or "other".
pub(crate) fn named(message: SessionMessage) -> String {
    match message {
        SessionMessage::Fetch(Fetched::Issue(item)) => format!("Issue #{}", item.number),
        SessionMessage::Fetch(Fetched::PullRequest(item)) => format!("PR #{}", item.number),
        SessionMessage::Refresh => "refresh".to_owned(),
        SessionMessage::SettingsChanged(settings, SavedBy::HandEdit) => {
            format!("settings of {}", settings.name)
        }
        SessionMessage::SettingsChanged(_, SavedBy::Window) => "switch".to_owned(),
        SessionMessage::Mark(Marked::Unseen { item, .. }) => format!("unseen #{}", item.number),
        SessionMessage::Stop => "stop".to_owned(),
        SessionMessage::Check { .. }
        | SessionMessage::Sync { .. }
        | SessionMessage::Mark(Marked::Seen { .. })
        | SessionMessage::Patched(..) => "other".to_owned(),
    }
}

/// A handle, what reaches the session through it, and what reaches the Mirror worker.
pub(crate) fn handle() -> (Fetching, Receiver<SessionMessage>, HeardRequests) {
    let (fetching, incoming, heard, _headless) = handle_with_headless();
    (fetching, incoming, heard)
}

/// A handle as `handle` makes it, and the writer of the PRs whose Conversation, as the session read it,
/// named no head.
pub(crate) fn handle_with_headless() -> (
    Fetching,
    Receiver<SessionMessage>,
    HeardRequests,
    LatestWriter<Vec<Item>>,
) {
    let (messages, incoming) = mpsc::channel();
    let (mirrors, heard) = HeardRequests::new();
    let (headless, reader) = latest::channel(Vec::new());
    (
        Fetching::new(messages, mirrors, reader),
        incoming,
        heard,
        headless,
    )
}
