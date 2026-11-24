//! The session's thread while the GitHub thread waits on a request: it takes each message at once, and
//! answers what needs no request (#997).

use super::*;
use crate::work::Unseen;
use std::sync::mpsc::Receiver;

/// Answers from `before` in order, then holds each question after them until the test lets go, and says
/// which it holds. Once let go, it can't reach GitHub.
struct Hanging {
    before: RefCell<VecDeque<Result<Reply, Unreachable>>>,
    held: Sender<String>,
    let_go: Receiver<()>,
}

impl Transport for Hanging {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "a test stops the GitHub thread with a panic in a request it holds"
    )]
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        if let Some(reply) = self.before.borrow_mut().pop_front() {
            return reply;
        }
        pass_on(&self.held, request.operation.to_owned());
        // The request fails once the test drops its end, as a test that panics does, and panics once the
        // test sends, which stops the GitHub thread.
        match self.let_go.recv() {
            Ok(()) => panic!("a panic in the GitHub thread"),
            Err(RecvError) => Err(Unreachable::Offline),
        }
    }
}

/// A request to GitHub that hangs, as `Hanging` says, with what the test holds of it.
struct Held {
    access: GitHubAccess,
    /// Names each question asked once `before` runs out: those that hang, and those after the test lets go.
    asked: Receiver<String>,
    /// Lets the questions go once dropped, and panics in the one held once it sends.
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

    asked.mark_unseen(
        gcmon(),
        Unseen {
            number: 7,
            archived: None,
        },
        None,
    );
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
            Account::Offline,
            Account::Reconnecting,
            Account::Offline,
            Account::Reconnecting,
            Account::Offline,
        ]
        .map(Status::from)
    );
    let left: Vec<Heard> = heard.try_iter().collect();
    let asked_after: Vec<String> = held.asked.try_iter().collect();
    assert_eq!((left, asked_after), (vec![], vec![]));
}

/// A session signed in, whose sync step of gcmon hangs, with a fetch of #7 the session passed on, which
/// waits in the GitHub thread's queue behind the step.
struct QueuedFetch {
    session: Session,
    /// The fetch's answer, which hasn't come yet.
    answer: Receiver<ItemFetch>,
    /// Names each question asked after the step's.
    asked: Receiver<String>,
    /// Fails the step once dropped, and panics in it once it sends.
    let_go: Sender<()>,
    /// The changes the session tells of, kept so the board can still pass them on.
    _changes: Receiver<Change>,
    _folder: TempPath,
}

fn fetch_queued_behind_a_step(name: &str) -> QueuedFetch {
    let (folders, folder) = temp_folders(name);
    let (board, changes) = changes_board();
    let Held {
        access,
        asked,
        let_go,
    } = hanging(vec![signed_in()]);
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);
    let fetching = session.fetching();
    fetching.sync(gcmon());
    drop(asked.recv_timeout(PLENTY).unwrap());

    // Online, so the session passes the fetch on, and the GitHub thread takes it after the step.
    let answer = fetching.fetch_unstored_later(Item {
        repository: gcmon(),
        number: 7,
    });
    // The session answers in order, so it has passed the fetch on once it tells of this mark. A fetch it
    // refuses it answers at once, so no answer yet means the GitHub thread holds it.
    fetching.mark_unseen(
        gcmon(),
        Unseen {
            number: 7,
            archived: None,
        },
        None,
    );
    drop(changes.recv_timeout(PLENTY).unwrap());
    assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
    QueuedFetch {
        session,
        answer,
        asked,
        let_go,
        _changes: changes,
        _folder: folder,
    }
}

#[test]
fn a_fetch_passed_on_before_github_went_away_asks_nothing_and_answers_offline() {
    let queued = fetch_queued_behind_a_step("held-fetch-raced");

    // The step fails, and the status says offline before the GitHub thread takes the fetch.
    drop(queued.let_go);
    let fetched = queued.answer.recv_timeout(PLENTY);
    queued.session.stop(PLENTY);

    assert_eq!(fetched, Ok(UNFETCHED_OFFLINE));
    let fetches = queued
        .asked
        .try_iter()
        .filter(|operation| operation == "FetchItem");
    assert_eq!(fetches.count(), 0);
}

/// The GitHub thread owns its queue, so what waits there goes with the thread, reply and all, and the fetch
/// stops waiting (#1076).
#[test]
fn a_fetch_waiting_on_the_github_thread_when_it_stops_goes_with_its_queue() {
    let queued = fetch_queued_behind_a_step("held-fetch-stopped");

    // The step panics, which stops the GitHub thread with the fetch still in its queue.
    queued.let_go.send(()).unwrap();
    let fetched = queued.answer.recv_timeout(PLENTY);
    queued.session.stop(PLENTY);

    assert_eq!(fetched, Err(RecvTimeoutError::Disconnected));
}

/// Asks the session for #7 of gcmon from a thread of its own, and waits for the answer. A session that
/// passed the fetch to the GitHub thread, which holds its request, fails the test at the wait.
fn fetch_beside(asked: &Fetching) -> Result<ItemFetch, RecvTimeoutError> {
    let fetching = asked.clone();
    beside(move || fetching.fetch_unstored(gcmon(), 7))
}
