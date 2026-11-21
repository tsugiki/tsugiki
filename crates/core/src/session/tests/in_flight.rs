//! The request to GitHub in flight in a running session: the GitHub thread tells the window what each
//! request asks as it goes, and nothing once it ends, so the status bar can show one GitHub is slow to
//! answer (#998).

use super::*;
use crate::github::in_flight::{Asking, InFlight};
use crate::github::testing::in_flight_heard;
use std::sync::mpsc::Receiver;

/// Tells the test each operation as it goes to GitHub, and answers it with what the test sends next.
struct Held {
    going: Sender<String>,
    answers: Receiver<Result<Reply, Unreachable>>,
}

impl Transport for Held {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        pass_on(&self.going, request.operation.to_owned());
        self.answers.recv().unwrap_or(Err(Unreachable::Offline))
    }
}

/// A session on GitHub through `transport`, whose board passes on the requests in flight, and what the
/// Mirror worker, which isn't there, would ask.
fn in_flight_session(
    name: &str,
    transport: impl Transport + 'static,
    announce: crate::github::in_flight::AnnounceInFlight,
) -> (Session, HeardRequests, TempPath) {
    let access = GitHubAccess {
        transport: Box::new(transport),
        token_from: SIGNED_IN,
    };
    let (folders, folder) = temp_folders(name);
    let board = quiet_board().with_in_flight(announce);
    let (requests, mirrors) = HeardRequests::new();
    let connections = Connections {
        mirrors: requests,
        ..reaching(access)
    };
    let session = Session::start(board, Ok(folders), connections, HOURLY);
    (session, mirrors, folder)
}

#[test]
fn a_request_is_in_flight_from_when_it_goes_until_github_answers() {
    let (going, gone) = mpsc::channel();
    let (answer, answers) = mpsc::channel();
    let (heard, in_flight) = mpsc::channel();
    let announce = Box::new(move |now: Option<&InFlight>| pass_on(&heard, now.cloned()));
    let before = OffsetDateTime::now_utc();
    let (session, _mirrors, _folder) =
        in_flight_session("in-flight-held", Held { going, answers }, announce);

    assert_eq!(gone.recv_timeout(PLENTY).unwrap(), "Viewer");
    let out = in_flight.recv_timeout(PLENTY).unwrap();
    let still_out = in_flight.try_recv();
    answer.send(signed_in()).unwrap();
    let ended = in_flight.recv_timeout(PLENTY).unwrap();
    drop(answer);
    session.stop(PLENTY);

    let out = out.unwrap();
    assert_eq!(out.asked, Asking::Status);
    assert!(
        before <= out.since && out.since <= OffsetDateTime::now_utc(),
        "{out:?}"
    );
    assert_eq!(still_out, Err(TryRecvError::Empty));
    assert_eq!(ended, None);
}

#[test]
fn each_request_the_github_thread_sends_names_what_it_asks() {
    // Any answer will do, as the test looks only at what each request asks.
    let (script, _asked) = Scripted::new(vec![signed_in(); 5]);
    let (announce, heard) = in_flight_heard();
    let (session, mirrors, _folder) = in_flight_session("in-flight-kinds", script, announce);

    let mut size_of = mirrors.sizes();

    session.fetching().fetch_issue(gcmon(), 7);
    let _unfetched = session.fetching().fetch_unstored(gcmon(), 8);
    drop(session.fetching().check(gcmon()));
    drop(size_of(&gcmon()));
    session.stop(PLENTY);

    let heard: Vec<_> = heard.try_iter().collect();
    let read = Asking::Read {
        repository: gcmon(),
        number: 7,
    };
    let fetch = Asking::Fetch {
        repository: gcmon(),
        number: 8,
    };
    let check = Asking::Check {
        repository: gcmon(),
    };
    let size = Asking::Size {
        repository: gcmon(),
    };
    assert_eq!(
        heard,
        [
            Some(Asking::Status),
            None,
            Some(read),
            None,
            Some(fetch),
            None,
            Some(check),
            None,
            Some(size),
            None,
        ]
    );
}
