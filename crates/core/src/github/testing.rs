use super::*;
use crate::token::GhOutput;

/// Answers every request the same way: the small builder ADR 0009 allows for rare cases.
pub(crate) struct Canned(pub(crate) Result<Reply, Unreachable>);

impl Transport for Canned {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        self.0.clone()
    }
}

pub(crate) fn answered(status: u16, body: &str) -> Canned {
    Canned(Ok(Reply {
        status,
        reset: None,
        date: None,
        body: body.to_owned(),
    }))
}

/// Answers from a list, in order, and records each operation and its variables for the test.
pub(crate) struct Scripted {
    replies: std::cell::RefCell<std::collections::VecDeque<Result<Reply, Unreachable>>>,
    asked: std::sync::mpsc::Sender<(String, Value)>,
}

impl Scripted {
    pub(crate) fn new(
        replies: Vec<Result<Reply, Unreachable>>,
    ) -> (Self, std::sync::mpsc::Receiver<(String, Value)>) {
        let (asked, seen) = std::sync::mpsc::channel();
        let replies = std::cell::RefCell::new(replies.into());
        (Self { replies, asked }, seen)
    }
}

impl Transport for Scripted {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        drop(
            self.asked
                .send((request.operation.to_owned(), request.variables.clone())),
        );
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(Unreachable::Offline))
    }
}

pub(crate) fn reply_ok(body: &str) -> Result<Reply, Unreachable> {
    Ok(Reply {
        status: 200,
        reset: None,
        date: None,
        body: body.to_owned(),
    })
}

/// `reply`, which GitHub dated `date`.
pub(crate) fn dated(
    reply: Result<Reply, Unreachable>,
    date: OffsetDateTime,
) -> Result<Reply, Unreachable> {
    reply.map(|reply| Reply {
        date: Some(date),
        ..reply
    })
}

/// GitHub's answer that `login` is signed in.
pub(crate) fn signed_in_as(login: &str) -> Result<Reply, Unreachable> {
    reply_ok(&format!(r#"{{"data":{{"viewer":{{"login":"{login}"}}}}}}"#))
}

pub(crate) fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: "gho_example".to_owned(),
    })
}

pub(crate) fn token() -> Token {
    crate::token::read_token(signed_in_gh).unwrap()
}
