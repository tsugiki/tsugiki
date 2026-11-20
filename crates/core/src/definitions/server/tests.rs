use super::*;

fn counted(done: u32) -> Indexing {
    Indexing::Building(Some(IndexedFiles { done, total: 452 }))
}

#[test]
fn a_new_state_or_the_first_count_is_news_and_a_new_count_only_once_a_second_has_passed() {
    let soon = Duration::from_millis(100);
    let news = |told, now| Indexing::news_since(now, told, soon);

    assert!(news(Indexing::Unreported, Indexing::Building(None)));
    assert!(news(Indexing::Building(None), counted(1)));
    assert!(!news(counted(1), counted(2)));
    assert!(counted(2).news_since(counted(1), NEWS_EVERY));
    assert!(news(counted(451), Indexing::Built));
    assert!(news(Indexing::Built, Indexing::Ended));
    assert!(!news(Indexing::Built, Indexing::Built));
}

/// What a server that is still loading sends: an event that isn't [`Event::Exited`].
fn loading() -> Event {
    Event::Status(Status {
        quiescent: false,
        message: None,
    })
}

fn crashed() -> Exit {
    Exit {
        code: Some(101),
        stderr_tail: "panicked".to_owned(),
    }
}

/// A server's events, which hold `sent` so far, and the sender of the rest.
fn events(sent: Vec<Event>) -> (Sender<Event>, Receiver<Event>) {
    let (sends, events) = mpsc::channel();
    for event in sent {
        sends.send(event).unwrap();
    }
    (sends, events)
}

/// How long a server in these tests takes to send an event once the caller waits for it.
const SOON: Duration = Duration::from_millis(100);

/// Sends `event` with `sends` [`SOON`] from now, from a thread of its own, then drops `sends`.
fn send_soon(sends: Sender<Event>, event: Event) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(SOON);
        sends.send(event).unwrap();
    })
}

#[test]
fn exit_by_reads_past_other_events_to_the_exit_and_is_none_if_the_events_end_or_time_is_up() {
    let start = Instant::now();
    let (_sends, exited) = events(vec![loading(), Event::Exited(crashed())]);
    let (gone, ended) = events(vec![loading()]);
    drop(gone);
    let (_sends, quiet) = events(vec![]);

    assert_eq!(exit_by(&exited, start + PATIENCE), Some(crashed()));
    assert_eq!(exit_by(&ended, start + PATIENCE), None);
    assert!(start.elapsed() < PATIENCE);
    assert_eq!(exit_by(&quiet, Instant::now() + SOON), None);
}

#[test]
fn exit_by_waits_for_an_exit_still_to_come() {
    let (sends, events) = events(vec![loading()]);
    let sender = send_soon(sends, Event::Exited(crashed()));

    let exit = exit_by(&events, Instant::now() + PATIENCE);

    assert_eq!(exit, Some(crashed()));
    sender.join().unwrap();
}

#[test]
fn a_stopping_server_is_waited_for_until_it_exits_or_its_events_end() {
    let start = Instant::now();
    let (_sends, exited) = events(vec![loading(), Event::Exited(crashed())]);
    let (gone, ended) = events(vec![loading()]);
    drop(gone);

    Stopping(exited).wait(start + PATIENCE);
    Stopping(ended).wait(start + PATIENCE);

    assert!(start.elapsed() < PATIENCE);
}

#[test]
fn a_stopping_server_is_waited_for_until_an_exit_still_to_come() {
    let start = Instant::now();
    let (sends, events) = events(vec![loading()]);
    let sender = send_soon(sends, Event::Exited(crashed()));

    Stopping(events).wait(start + PATIENCE);

    assert!(start.elapsed() >= SOON);
    sender.join().unwrap();
}

#[test]
fn a_server_is_quiescent_once_it_says_so_and_has_stopped_if_it_exits_first() {
    let done = Event::Status(Status {
        quiescent: true,
        message: None,
    });
    // An exit after the quiescent status, which a wait that read past that status would give.
    let (_sends, ready) = events(vec![loading(), done, Event::Exited(crashed())]);
    let (_sends, exited) = events(vec![loading(), Event::Exited(crashed())]);

    assert_eq!(quiescent(&ready, Instant::now() + PATIENCE), None);
    assert_eq!(
        quiescent(&exited, Instant::now() + PATIENCE),
        Some(crashed())
    );
}

#[test]
fn once_the_deadline_has_passed_no_queued_event_is_read() {
    let (_sends, events) = events(vec![loading(), Event::Exited(crashed())]);
    let passed = Instant::now();

    assert_eq!(exit_by(&events, passed), None);
    assert_eq!(quiescent(&events, passed), None);
    assert_eq!(events.try_recv(), Ok(loading()));
}

#[test]
fn a_stopping_server_is_waited_for_until_the_deadline_while_it_floods_its_events() {
    // How long the server floods its events, unless nothing reads them before then.
    let flood = 20 * SOON;
    let start = Instant::now();
    // A bound keeps the queue from growing without limit, and full while the server floods it.
    let (sends, events) = mpsc::sync_channel(10_000);
    let flooder = thread::spawn(move || {
        while start.elapsed() < flood {
            if sends.send(loading()).is_err() {
                break;
            }
        }
    });

    Stopping(events).wait(start + SOON);

    let waited = start.elapsed();
    assert!(waited >= SOON, "{waited:?}");
    assert!(waited < flood, "{waited:?}");
    flooder.join().unwrap();
}

#[test]
fn a_log_whose_folder_cant_be_made_is_none_and_the_server_runs_without_it() {
    let root = crate::testing::temp_path("definitions-log-blocked");
    fs::create_dir_all(&root).unwrap();
    // A file where the server's folder would go.
    let blocked = root.join("servers");
    fs::write(&blocked, "").unwrap();

    let log = new_log(&blocked.join("ty").join("ty.log"));

    assert!(log.is_none());
}
