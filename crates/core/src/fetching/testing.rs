use std::sync::mpsc::{self, Receiver};

use super::{
    AnsweredMark, Fetched, Fetching, Headless, MarkReply, Marked, Queued, ScreenMove,
    SessionMessage,
};
use crate::error::{Error, ErrorKind};
use crate::mirror::HeardRequests;
use crate::repository_settings::SavedBy;

/// What each call of `Fetching` that queues a job answers, for a test to compare with.
pub(crate) const QUEUED: Queued = Queued(());

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
        SessionMessage::Mark(marked) => marked_name(&marked),
        // A mark the window waits on says so, as it answers once the store has kept it (#489).
        SessionMessage::MarkAnswered { marked, .. } => {
            format!("{}, answered", marked_name(&marked))
        }
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
        | SessionMessage::ShowMuted { .. }
        | SessionMessage::Patched(..)
        | SessionMessage::Saved(_) => "other".to_owned(),
    }
}

/// What `marked` does, and to which item, as a test names it.
fn marked_name(marked: &Marked) -> String {
    match marked {
        Marked::Seen { mark, .. } => format!("seen {:?}", mark.numbers),
        Marked::Unseen { number, .. } => format!("unseen #{number}"),
        Marked::Archived { key, .. } => format!("archived #{}", key.number()),
        Marked::Unarchived { key, .. } => format!("unarchived #{}", key.number()),
        Marked::Muted { key, .. } => format!("muted #{}", key.number()),
        Marked::Unmuted { key, .. } => format!("unmuted #{}", key.number()),
        Marked::AuthorMuted { author, .. } => format!("muted author {}", author.as_str()),
        Marked::AuthorUnmuted { author, .. } => format!("unmuted author {}", author.as_str()),
        Marked::Pinned { number, .. } => format!("pinned #{number}"),
        Marked::Unpinned { number, .. } => format!("unpinned #{number}"),
    }
}

/// A reply to a mark the window waits on, and where a test hears the answer it hands on (#489).
pub(crate) fn replied() -> (MarkReply, Receiver<Result<(), Error>>) {
    let (heard, answer) = mpsc::channel();
    let reply = MarkReply::new(move |result| {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only once the test stopped listening, and then nobody wants the answer"
        )]
        let _ = heard.send(result);
    });
    (reply, answer)
}

/// Mark Unseen on Issue or PR `number`, which no Archived item holds.
pub(crate) fn unseen(number: u32) -> AnsweredMark {
    AnsweredMark::Unseen {
        number,
        archived: None,
    }
}

/// What the session answers the mark `mark` sends with the reply it's handed, once it answers, as the
/// window waits for it. A reply answers, if only as it drops, so the last arm is there for the types.
pub(crate) fn kept(mark: impl FnOnce(MarkReply)) -> Result<(), Error> {
    let (reply, answer) = replied();
    mark(reply);
    (answer.recv())
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::Offline, "the reply went unanswered")))
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
