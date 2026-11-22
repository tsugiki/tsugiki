use std::sync::mpsc::Receiver;

use serde_json::{Value, json};

use super::*;
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_requests::NodeId;
use crate::github::testing::{COMMENT_ID, RATE_LIMIT, Scripted, answer, github, thanks};
use crate::latest;
use crate::read_only::TokenSeen;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, empty_conversation, gcmon, listed, listing};
use crate::token::TokenKind;

/// GitHub's answer to a comment it made.
fn comment_made() -> Value {
    json!({ "addComment": { "commentEdge": { "node": { "id": "IC_1" } } } })
}

/// A client whose transport answers one write with [`comment_made`], and what it was sent.
fn answering_once() -> (GitHub, Receiver<(String, Value)>) {
    let (transport, asked) = Scripted::new(vec![answer(comment_made(), RATE_LIMIT)]);
    (github(transport), asked)
}

/// A read of an item whose ID GitHub gave as `id`, or of one kept from before Tsugiki asked.
fn read_with(id: Option<&str>) -> IssueContent {
    IssueContent {
        id: id.map(|id| NodeId::new(id.to_owned())),
        ..empty_conversation()
    }
}

/// A store that has read item #7 of CPython, whose ID is `I_7`, and item #7 of gcmon, whose ID is `I_gcmon`.
fn has_read_both(name: &str) -> OpenStore {
    let db = OpenStore::new(name);
    let read = |repository: &RepositoryName, id| {
        db.store
            .save_issue_content(repository, 7, read_with(Some(id)))
    };
    read(&cpython(), "I_7").unwrap();
    read(&gcmon(), "I_gcmon").unwrap();
    db
}

/// "Thanks" to item `number` of `repository`.
fn thanks_to(repository: RepositoryName, number: u32) -> ItemWrite<'static> {
    ItemWrite {
        repository,
        number,
        write: thanks(),
        selection: COMMENT_ID,
    }
}

/// `repository`'s settings once the user chose Read-only for it.
fn chosen_read_only(repository: RepositoryName) -> RepositorySettings {
    RepositorySettings {
        read_only: true,
        ..RepositorySettings::new(repository)
    }
}

#[test]
fn a_write_to_a_repository_the_user_chose_read_only_for_since_the_control_showed_says_why_and_sends_nothing()
 {
    let db = has_read_both("write-chosen-read-only");
    let (github, asked) = answering_once();
    let (mut settings, view) = latest::channel(listing(vec![RepositorySettings::new(cpython())]));
    settings.set(&listing(vec![chosen_read_only(cpython())]));

    let sent = send::<Value>(&github, &view, &db.reader, thanks_to(cpython(), 7));

    let problem = sent.unwrap_err();
    assert_eq!(problem.kind, ErrorKind::WriteRefused);
    assert_eq!(
        problem.message,
        "Tsugiki sent nothing, as you chose Read-only in python/cpython's settings."
    );
    assert!(asked.try_recv().is_err());
    db.close();
}

#[test]
fn a_write_to_a_repository_the_store_marks_read_only_for_the_token_says_why_and_sends_nothing() {
    let db = has_read_both("write-token-read-only");
    let token = TokenSeen {
        kind: TokenKind::Classic,
        expires: None,
    };
    assert!(db.store.mark_read_only(token, &cpython()).unwrap());
    let (github, asked) = answering_once();

    let sent = send::<Value>(
        &github,
        &listed(vec![]),
        &db.reader,
        thanks_to(cpython(), 7),
    );

    let problem = sent.unwrap_err();
    assert_eq!(problem.kind, ErrorKind::WriteRefused);
    assert_eq!(
        problem.message,
        "Tsugiki sent nothing, as your token can't write to python/cpython."
    );
    assert!(asked.try_recv().is_err());
    db.close();
}

#[test]
fn a_write_to_a_writable_repository_names_the_item_by_the_id_its_last_read_gave_there() {
    let db = has_read_both("write-writable");
    let (github, asked) = answering_once();
    let settings = listed(vec![chosen_read_only(cpython())]);

    let answered: Value = send(&github, &settings, &db.reader, thanks_to(gcmon(), 7)).unwrap();

    assert_eq!(answered["addComment"], comment_made()["addComment"]);
    let input = json!({ "input": { "subjectId": "I_gcmon", "body": "Thanks" } });
    assert_eq!(asked.recv().unwrap(), ("AddComment".to_owned(), input));
    db.close();
}

#[test]
fn a_write_to_an_item_the_store_has_no_id_for_says_to_open_it_again_and_sends_nothing() {
    let db = has_read_both("write-unread");
    db.store
        .save_issue_content(&cpython(), 8, read_with(None))
        .unwrap();

    for number in [8, 9] {
        let (github, asked) = answering_once();
        let sent = send::<Value>(
            &github,
            &listed(vec![]),
            &db.reader,
            thanks_to(cpython(), number),
        );

        let problem = sent.unwrap_err();
        assert_eq!(problem.kind, ErrorKind::Conflict);
        assert_eq!(
            problem.message,
            format!(
                "Tsugiki sent nothing, as it hasn't read #{number} of python/cpython from GitHub yet. \
                 Open it again, and send once it has."
            )
        );
        assert!(asked.try_recv().is_err());
    }
    db.close();
}
