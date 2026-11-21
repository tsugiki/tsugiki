//! The session's thread while the GitHub thread waits on a request: it takes each message at once, and
//! answers what needs no request (#997).

use super::*;
use std::sync::mpsc::Receiver;

/// Answers from `before` in order, then holds each question after them until the test lets go, and says
/// which it holds. Once let go, it can't reach GitHub.
struct Hanging {
    before: RefCell<VecDeque<Result<Reply, Unreachable>>>,
    held: Sender<String>,
    let_go: Receiver<()>,
}

impl Transport for Hanging {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        if let Some(reply) = self.before.borrow_mut().pop_front() {
            return reply;
        }
        pass_on(&self.held, request.operation.to_owned());
        // The test lets go when it drops its end, or when it panics.
        let RecvError = self.let_go.recv().unwrap_err();
        Err(Unreachable::Offline)
    }
}

/// A request to GitHub that hangs, as `Hanging` says, with what the test holds of it.
struct Held {
    access: GitHubAccess,
    /// Says each question that hangs.
    asked: Receiver<String>,
    /// Lets the questions go once dropped.
    let_go: Sender<()>,
}

/// GitHub's access through a transport that answers `before`, then hangs.
fn hanging(before: Vec<Result<Reply, Unreachable>>) -> Held {
    let (held, asked) = mpsc::channel();
    let (let_go, waiting) = mpsc::channel();
    let transport = Hanging {
        before: RefCell::new(before.into()),
        held,
        let_go: waiting,
    };
    let access = GitHubAccess {
        transport: Box::new(transport),
        token_from: SIGNED_IN,
    };
    Held {
        access,
        asked,
        let_go,
    }
}

#[test]
fn while_a_sync_step_hangs_the_session_keeps_marks_and_patch_ids_and_tells_of_a_hand_edit() {
    let (folders, _folder) = temp_folders("held-sync-step");
    store_pr_8_reviewed(&folders);
    let (board, changes) = changes_board();
    let held = hanging(vec![signed_in()]);
    let session = Session::start(board, Ok(folders.clone()), reaching(held.access), HOURLY);
    let asked = session.fetching();
    asked.sync(gcmon());
    drop(held.asked.recv_timeout(PLENTY).unwrap());

    asked.mark_unseen(gcmon(), 7, None);
    let marked = changes.recv_timeout(PLENTY).unwrap();
    asked.settings_changed(bugs_saved(OnWorkbench::Off, SavedBy::HandEdit));
    let edited = changes.recv_timeout(PLENTY).unwrap();
    let refused = UncheckedCommit {
        of: on_main("0123abc"),
        reason: CantCheck::Refused,
    };
    asked.patch_ids_keeper()(&gcmon(), PatchReads::Unchecked(vec![refused]));
    let patched = changes.recv_timeout(PLENTY).unwrap();
    let activity = session.reader().work_facts(&gcmon()).unwrap().activity;
    // The step still hangs.
    drop(held.let_go);
    session.stop(PLENTY);

    let items = [marked.items, edited.items, patched.items];
    assert_eq!(items, [vec![7], vec![], vec![8]]);
    assert_eq!(activity.marked_unseen.keys().collect::<Vec<_>>(), [&7]);
}

#[test]
fn while_a_refresh_waits_on_github_the_session_answers_a_fetch_and_drops_another_refresh() {
    let (folders, _folder) = temp_folders("held-refresh");
    // GitHub can't be reached at the start, and hangs once the user clicks Refresh.
    let held = hanging(vec![Err(Unreachable::Offline)]);
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        reaching(held.access),
        HOURLY,
    );
    let asked = session.fetching();
    asked.refresh();
    let refreshing = held.asked.recv_timeout(PLENTY).unwrap();

    let fetching = asked.clone();
    let (answer, answered) = mpsc::channel();
    #[expect(
        unused_results,
        reason = "it ends with the answer, or once the test lets the request go"
    )]
    thread::spawn(move || answer.send(fetching.fetch_unstored(gcmon(), 7)));
    let fetched = answered.recv_timeout(PLENTY);
    asked.refresh();
    asked.refresh();
    drop(held.let_go);
    session.stop(PLENTY);

    let offline = ItemFetch::Unfetched {
        why: ItemUnfetched::Offline,
    };
    assert_eq!(fetched, Ok(offline));
    assert_eq!(refreshing, "Viewer");
    // The refresh still waiting covers the clicks after it.
    assert_eq!(
        held.asked.try_iter().collect::<Vec<_>>(),
        Vec::<String>::new()
    );
}
