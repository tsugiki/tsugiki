use std::sync::mpsc::{self, Receiver};

use super::{
    Fetched, Fetching, Headless, Item, ItemFetch, Marked, Queued, ScreenMove, SessionMessage,
};
use crate::mirror::HeardRequests;
use crate::repository_settings::SavedBy;

/// What each call of `Fetching` that queues a job answers, for a test to compare with.
pub(crate) const QUEUED: Queued = Queued(());

impl Fetching {
    /// Asks the session for `item` as `fetch_unstored` does, without waiting: the answer comes on the
    /// receiver. A test can then send the session more, and know the fetch reached it first.
    pub(crate) fn fetch_unstored_later(&self, item: Item) -> Receiver<ItemFetch> {
        let (reply, answer) = mpsc::channel();
        self.to_session(SessionMessage::FetchUnstored { item, reply });
        answer
    }
}

/// `message` as a test names it: what it reads, refreshes, changes or marks, or "other".
pub(crate) fn named(message: SessionMessage) -> String {
    match message {
        SessionMessage::Fetch(Fetched::Issue(item)) => format!("Issue #{}", item.number),
        SessionMessage::Fetch(Fetched::PullRequest(item)) => format!("PR #{}", item.number),
        SessionMessage::Refresh => "refresh".to_owned(),
        SessionMessage::FetchUnstored { item, .. } => format!("fetch #{}", item.number),
        SessionMessage::SettingsChanged(save) => match save.by() {
            SavedBy::HandEdit => format!("settings of {}", save.saved().name),
            SavedBy::Window => "switch".to_owned(),
        },
        SessionMessage::Mark(Marked::Unseen { number, .. }) => format!("unseen #{number}"),
        SessionMessage::Stop => "stop".to_owned(),
        SessionMessage::Screen(moved) => match moved {
            ScreenMove::Opened(Fetched::Issue(item)) => format!("opened Issue #{}", item.number),
            ScreenMove::Opened(Fetched::PullRequest(item)) => {
                format!("opened PR #{}", item.number)
            }
            ScreenMove::Hid(item) => format!("hid #{}", item.number),
            ScreenMove::Showed(item) => format!("showed #{}", item.number),
            ScreenMove::Closed(item) => format!("closed #{}", item.number),
            ScreenMove::WindowHid => "window hid".to_owned(),
            ScreenMove::WindowShowed => "window showed".to_owned(),
            ScreenMove::WindowLoaded => "window loaded".to_owned(),
        },
        SessionMessage::Check { .. }
        | SessionMessage::Sync { .. }
        | SessionMessage::Mark(
            Marked::Seen { .. }
            | Marked::Archived { .. }
            | Marked::Unarchived { .. }
            | Marked::Muted { .. }
            | Marked::Unmuted { .. }
            | Marked::AuthorMuted { .. }
            | Marked::AuthorUnmuted { .. },
        )
        | SessionMessage::Patched(..)
        | SessionMessage::Saved(_) => "other".to_owned(),
    }
}

/// A handle, what reaches the session through it, and what reaches the Mirror worker.
pub(crate) fn handle() -> (Fetching, Receiver<SessionMessage>, HeardRequests) {
    let (fetching, incoming, heard, _headless) = handle_with_headless();
    (fetching, incoming, heard)
}

/// A handle as `handle` makes it, and the PRs whose Conversation, as the session read it, named no head,
/// which the handle shares.
pub(crate) fn handle_with_headless() -> (Fetching, Receiver<SessionMessage>, HeardRequests, Headless)
{
    let (messages, incoming) = mpsc::channel();
    let (mirrors, heard) = HeardRequests::new();
    let headless = Headless::default();
    let fetching = Fetching::new(messages, mirrors, headless.clone());
    (fetching, incoming, heard, headless)
}
