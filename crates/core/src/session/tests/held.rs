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
    /// Names each question asked once `before` runs out: those that hang, and those after the test lets go.
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

    asked.mark_answered(gcmon(), unseen(7), replied().0);
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

    asked.refresh();
    asked.refresh();
    let fetched = fetch_beside(&asked);
    drop(held.let_go);
    session.stop(PLENTY);

    assert_eq!(fetched, Ok(UNFETCHED_OFFLINE));
    assert_eq!(refreshing, "Viewer");
    // The refresh still waiting covers the clicks after it.
    assert_eq!(
        held.asked.try_iter().collect::<Vec<_>>(),
        Vec::<String>::new()
    );
}

#[test]
fn several_refresh_clicks_while_offline_ask_github_once_and_a_click_after_offline_asks_again() {
    let (folders, _folder) = temp_folders("held-clicks");
    let (board, heard) = heard_board(|_| {}, |_| {});
    // GitHub can't be reached at the start, and hangs once the user clicks Refresh.
    let held = hanging(vec![Err(Unreachable::Offline)]);
    let session = Session::start(board, Ok(folders.clone()), reaching(held.access), HOURLY);
    let asked = session.fetching();
    let mut statuses = vec![next_status(&heard)];

    asked.refresh();
    let first = held.asked.recv_timeout(PLENTY).unwrap();
    statuses.push(next_status(&heard));
    // The status bar says "Connecting…" while the user clicks twice more.
    asked.refresh();
    asked.refresh();
    // The session answers in order, so it has taken both clicks once it answers this.
    let fetched = fetch_beside(&asked);
    // GitHub fails the request, and fails at once from now on.
    drop(held.let_go);
    statuses.push(next_status(&heard));

    asked.refresh();
    let again = held.asked.recv_timeout(PLENTY).unwrap();
    statuses.push(next_status(&heard));
    statuses.push(next_status(&heard));
    session.stop(PLENTY);

    assert_eq!((first, again), ("Viewer".to_owned(), "Viewer".to_owned()));
    assert_eq!(fetched, Ok(UNFETCHED_OFFLINE));
    assert_eq!(
        statuses,
        [
            Status::Offline,
            Status::Reconnecting,
            Status::Offline,
            Status::Reconnecting,
            Status::Offline,
        ]
    );
    let left: Vec<Heard> = heard.try_iter().collect();
    let asked_after: Vec<String> = held.asked.try_iter().collect();
    assert_eq!((left, asked_after), (vec![], vec![]));
}

#[test]
fn a_fetch_passed_on_before_github_went_away_asks_nothing_and_answers_offline() {
    let (folders, _folder) = temp_folders("held-fetch-raced");
    let (board, changes) = changes_board();
    let held = hanging(vec![signed_in()]);
    let session = Session::start(board, Ok(folders), reaching(held.access), HOURLY);
    let asked = session.fetching();
    asked.sync(gcmon());
    drop(held.asked.recv_timeout(PLENTY).unwrap());

    // Online, so the session passes the fetch on, and the GitHub thread takes it after the step.
    let answer = asked.fetch_unstored_later(Item {
        repository: gcmon(),
        number: 7,
    });
    // The session answers in order, so it has passed the fetch on once it tells of this mark. A fetch it
    // refuses it answers at once, so no answer yet means the GitHub thread holds it.
    asked.mark_answered(gcmon(), unseen(7), replied().0);
    drop(changes.recv_timeout(PLENTY).unwrap());
    let before = answer.try_recv();
    // The step fails, and the status says offline before the GitHub thread takes the fetch.
    drop(held.let_go);
    let fetched = answer.recv_timeout(PLENTY);
    session.stop(PLENTY);

    assert_eq!(before, Err(TryRecvError::Empty));
    assert_eq!(fetched, Ok(UNFETCHED_OFFLINE));
    let fetches = held
        .asked
        .try_iter()
        .filter(|operation| operation == "FetchItem");
    assert_eq!(fetches.count(), 0);
}

/// Asks the session for #7 of gcmon from a thread of its own, and waits for the answer. A session that
/// passed the fetch to the GitHub thread, which holds its request, fails the test at the wait.
fn fetch_beside(asked: &Fetching) -> Result<ItemFetch, RecvTimeoutError> {
    let fetching = asked.clone();
    let (answer, answered) = mpsc::channel();
    #[expect(
        unused_results,
        reason = "it ends with the answer, or once the test lets the request go"
    )]
    thread::spawn(move || answer.send(fetching.fetch_unstored(gcmon(), 7)));
    answered.recv_timeout(PLENTY)
}
