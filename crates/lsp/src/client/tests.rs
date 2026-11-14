use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{Event, Exit, Init, Pending, RequestError, Server};
use crate::testing::{Fake, PATIENCE, connect, initialize, initialized, uri};
use crate::types::{
    Definitions, Diagnostics, Language, Location, LocationLink, Position, Progress, ProgressToken,
    Range, ServerInfo, WorkDone,
};

fn next_event(events: &Receiver<Event>) -> Event {
    events.recv_timeout(PATIENCE).unwrap()
}

fn range(line: u32, from: u32, to: u32) -> Range {
    Range {
        start: Position {
            line,
            character: from,
        },
        end: Position {
            line,
            character: to,
        },
    }
}

fn json_range(line: u32, from: u32, to: u32) -> Value {
    json!({"start": {"line": line, "character": from}, "end": {"line": line, "character": to}})
}

fn somewhere(server: &Server) -> Pending<Definitions> {
    server.definition(
        &uri("file:///w/a.py"),
        Position {
            line: 0,
            character: 0,
        },
    )
}

/// Asks for a definition in `a.py`, checks what the server got, and answers it with `reply`.
fn answered(server: &Server, fake: &mut Fake, reply: Value) -> Pending<Definitions> {
    let pending = server.definition(
        &uri("file:///w/a.py"),
        Position {
            line: 3,
            character: 8,
        },
    );
    let asked = fake.expect("textDocument/definition");
    assert_eq!(
        asked["params"],
        json!({"textDocument": {"uri": "file:///w/a.py"}, "position": {"line": 3, "character": 8}})
    );
    fake.reply(&asked, reply);
    pending
}

fn definition(server: &Server, fake: &mut Fake, reply: Value) -> Result<Definitions, RequestError> {
    answered(server, fake, reply).wait()
}

#[test]
fn initialize_says_what_the_client_can_do_then_sends_initialized() {
    let (starting, _events, mut fake) = connect();
    let init = Init {
        root: uri("file:///work/lookup"),
        options: json!({"logLevel": "info"}),
        settings: json!({}),
    };
    let initializing = initialize(starting, init, PATIENCE);
    let asked = fake.expect("initialize");
    let params = &asked["params"];
    assert!(params["processId"].is_u64());
    assert_eq!(params["rootUri"], "file:///work/lookup");
    assert_eq!(params["workspaceFolders"][0]["uri"], "file:///work/lookup");
    assert_eq!(params["initializationOptions"], json!({"logLevel": "info"}));
    assert_eq!(params["clientInfo"]["name"], "Tsugiki");
    let capabilities = &params["capabilities"];
    assert_eq!(
        capabilities["general"]["positionEncodings"],
        json!(["utf-16"])
    );
    assert_eq!(capabilities["workspace"]["configuration"], true);
    assert_eq!(
        capabilities["textDocument"]["definition"]["linkSupport"],
        true
    );
    assert_eq!(capabilities["window"]["workDoneProgress"], true);

    fake.reply(
        &asked,
        json!({"capabilities": {}, "serverInfo": {"name": "ty", "version": "0.0.103"}}),
    );
    let (_server, info) = initializing.join().unwrap().unwrap();
    assert_eq!(
        info,
        Some(ServerInfo {
            name: "ty".into(),
            version: Some("0.0.103".into())
        })
    );
    assert_eq!(fake.expect("initialized")["params"], json!({}));
}

#[test]
fn a_failed_initialize_sends_no_initialized_and_kills_the_server() {
    let (starting, _events, mut fake) = connect();
    let init = Init {
        root: uri("file:///w"),
        options: json!(null),
        settings: json!({}),
    };
    let initializing = initialize(starting, init, PATIENCE);
    let asked = fake.expect("initialize");
    fake.send(&json!({"jsonrpc": "2.0", "id": asked["id"], "error": {"code": -32603, "message": "no root"}}));
    let Err(error) = initializing.join().unwrap() else {
        panic!("initialized")
    };
    assert!(
        matches!(&error, RequestError::Server { code: -32603, message } if message == "no root")
    );
    assert_eq!(
        error.to_string(),
        "the server answered error -32603: no root"
    );

    fake.killed.recv_timeout(PATIENCE).unwrap();
    fake.reap(None);
    assert!(fake.ended());
}

#[test]
fn a_server_that_doesn_t_answer_initialize_in_time_is_killed() {
    let (starting, _events, mut fake) = connect();
    let init = Init {
        root: uri("file:///w"),
        options: json!(null),
        settings: json!({}),
    };
    let initializing = initialize(starting, init, Duration::from_millis(50));
    let _asked = fake.expect("initialize");
    let Err(error) = initializing.join().unwrap() else {
        panic!("initialized")
    };
    assert!(matches!(error, RequestError::TimedOut));
    assert_eq!(error.to_string(), "the server didn't answer in time");
    fake.killed.recv_timeout(PATIENCE).unwrap();
}

#[test]
fn workspace_configuration_is_answered_from_the_settings_section_by_section() {
    let environment = json!({"root": ["Lib"], "python-version": "3.15"});
    let settings = json!({"ty": {"environment": environment}});
    let (_server, _events, mut fake) = initialized(settings.clone());
    fake.send(&json!({
        "jsonrpc": "2.0",
        "id": "c1",
        "method": "workspace/configuration",
        "params": {"items": [
            {"section": "ty"},
            {"section": "ty.environment.root"},
            {"scopeUri": "file:///work/lookup/Lib", "section": "ty.environment"},
            {"section": "rust-analyzer"},
            {},
        ]},
    }));
    let answer = fake.next();
    assert_eq!(answer["id"], "c1");
    assert_eq!(
        answer["result"],
        json!([settings["ty"], ["Lib"], environment, null, settings])
    );
}

#[test]
fn workspace_folders_are_the_root_the_client_gave() {
    let (_server, _events, mut fake) = initialized(json!({}));
    fake.send(&json!({"jsonrpc": "2.0", "id": 5, "method": "workspace/workspaceFolders"}));
    assert_eq!(
        fake.next()["result"],
        json!([{"uri": "file:///work/lookup", "name": "file:///work/lookup"}])
    );
}

#[test]
fn a_request_from_the_server_the_client_can_t_answer_gets_an_error() {
    let (_server, _events, mut fake) = initialized(json!({}));
    fake.send(
        &json!({"jsonrpc": "2.0", "id": 1, "method": "workspace/configuration", "params": {}}),
    );
    assert_eq!(fake.next()["error"]["code"], -32602);
    fake.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "workspace/applyEdit", "params": {}}));
    let refused = fake.next();
    assert_eq!(refused["id"], 2);
    assert_eq!(refused["error"]["code"], -32601);
    assert_eq!(
        refused["error"]["message"],
        "Tsugiki doesn't answer workspace/applyEdit"
    );
}

#[test]
fn the_client_agrees_to_progress_and_registrations() {
    let (_server, _events, mut fake) = initialized(json!({}));
    let methods = [
        "window/workDoneProgress/create",
        "client/registerCapability",
        "client/unregisterCapability",
    ];
    for (id, method) in methods.into_iter().enumerate() {
        fake.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}}));
        assert_eq!(
            fake.next(),
            json!({"jsonrpc": "2.0", "id": id, "result": null}),
            "{method}"
        );
    }
}

#[test]
fn a_document_opens_changes_with_counted_versions_and_closes() {
    let (server, _events, mut fake) = initialized(json!({}));
    let mut document = server.open(uri("file:///w/a.py"), Language::Python, "x = 1\n");
    assert_eq!(
        fake.expect("textDocument/didOpen")["params"],
        json!({"textDocument": {"uri": "file:///w/a.py", "languageId": "python", "version": 1, "text": "x = 1\n"}})
    );
    assert_eq!(document.uri(), &uri("file:///w/a.py"));

    for (version, text) in [(2, "x = 2\n"), (3, "x = 3\n")] {
        document.change(text);
        assert_eq!(document.version(), version);
        assert_eq!(
            fake.expect("textDocument/didChange")["params"],
            json!({"textDocument": {"uri": "file:///w/a.py", "version": version}, "contentChanges": [{"text": text}]})
        );
    }

    drop(document);
    assert_eq!(
        fake.expect("textDocument/didClose")["params"],
        json!({"textDocument": {"uri": "file:///w/a.py"}})
    );
}

#[test]
fn a_definition_with_one_hit_several_or_none() {
    let (server, _events, mut fake) = initialized(json!({}));
    let one = json!({"uri": "file:///w/b.py", "range": json_range(0, 4, 7)});
    let location = Location {
        uri: uri("file:///w/b.py"),
        range: range(0, 4, 7),
    };

    assert_eq!(
        definition(&server, &mut fake, one.clone()).unwrap(),
        Definitions::Locations(vec![location.clone()])
    );
    assert_eq!(
        definition(&server, &mut fake, json!([one, one])).unwrap(),
        Definitions::Locations(vec![location.clone(), location])
    );
    assert!(
        definition(&server, &mut fake, Value::Null)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_definition_as_links() {
    let (server, _events, mut fake) = initialized(json!({}));
    let link = json!({
        "targetUri": "file:///w/Include/object.h",
        "targetRange": json_range(10, 0, 30),
        "targetSelectionRange": json_range(10, 12, 20),
    });
    assert_eq!(
        definition(&server, &mut fake, json!([link])).unwrap(),
        Definitions::Links(vec![LocationLink {
            origin_selection_range: None,
            target_uri: uri("file:///w/Include/object.h"),
            target_range: range(10, 0, 30),
            target_selection_range: range(10, 12, 20),
        }])
    );
}

#[test]
fn another_thread_cancels_a_request_one_waits_for() {
    let (server, _events, mut fake) = initialized(json!({}));
    let pending = somewhere(&server);
    let asked = fake.expect("textDocument/definition");
    let canceller = pending.canceller();
    let waiting = thread::spawn(move || pending.wait());

    canceller.cancel();

    assert_eq!(
        fake.expect("$/cancelRequest")["params"],
        json!({"id": asked["id"]})
    );
    let error = waiting.join().unwrap().unwrap_err();
    assert!(matches!(error, RequestError::Cancelled));
}

#[test]
fn a_cancelled_request_fails_and_the_server_s_late_answer_is_dropped() {
    let (server, _events, mut fake) = initialized(json!({}));
    let pending = somewhere(&server);
    let asked = fake.expect("textDocument/definition");
    assert!(pending.wait_timeout(Duration::from_millis(10)).is_none());

    pending.cancel();
    assert_eq!(
        fake.expect("$/cancelRequest")["params"],
        json!({"id": asked["id"]})
    );
    let error = pending.wait_timeout(PATIENCE).unwrap().unwrap_err();
    assert!(matches!(error, RequestError::Cancelled));
    assert_eq!(error.to_string(), "the request was cancelled");
    // Nothing waits any more, so a second cancel sends nothing: the next message is the next request.
    pending.cancel();

    fake.send(&json!({"jsonrpc": "2.0", "id": asked["id"], "error": {"code": -32800, "message": "cancelled"}}));
    fake.send(&json!({"jsonrpc": "2.0", "id": "not ours", "result": null}));
    assert!(
        definition(&server, &mut fake, Value::Null)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn shutdown_sends_exit_once_the_server_agrees_and_the_exit_is_reported() {
    let (server, events, mut fake) = initialized(json!({}));
    let pending = server.shutdown();
    let asked = fake.expect("shutdown");
    assert!(asked.get("params").is_none(), "{asked}");
    fake.reply(&asked, Value::Null);
    assert!(fake.expect("exit").get("params").is_none());
    pending.wait().unwrap();

    fake.exit(Some(0));
    assert_eq!(
        next_event(&events),
        Event::Exited(Exit {
            code: Some(0),
            stderr_tail: String::new()
        })
    );
    // Asked after the end, a request fails at once.
    assert!(matches!(
        somewhere(&server).wait(),
        Err(RequestError::Exited)
    ));
    drop(server);
    assert!(
        fake.killed
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}

#[test]
fn a_server_dropped_as_it_exits_gets_to_exit_and_its_exit_is_reported() {
    let (server, events, mut fake) = initialized(json!({}));
    let pending = server.shutdown();
    let asked = fake.expect("shutdown");
    fake.reply(&asked, Value::Null);
    let _exit = fake.expect("exit");
    pending.wait().unwrap();
    drop(server);

    fake.write_errors("bye\n");
    fake.exit(Some(0));
    assert_eq!(
        next_event(&events),
        Event::Exited(Exit {
            code: Some(0),
            stderr_tail: "bye\n".into()
        })
    );
    assert!(
        fake.killed
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}

#[test]
fn a_server_that_agreed_to_stop_and_doesn_t_is_killed_once_dropped() {
    let (server, _events, mut fake) = initialized(json!({}));
    let pending = server.shutdown();
    let asked = fake.expect("shutdown");
    fake.reply(&asked, Value::Null);
    let _exit = fake.expect("exit");
    pending.wait().unwrap();
    drop(server);
    fake.killed.recv_timeout(PATIENCE).unwrap();
}

#[test]
fn a_server_that_exits_during_a_request_fails_it_and_reports_its_error_output() {
    let (server, events, mut fake) = initialized(json!({}));
    let pending = somewhere(&server);
    let _asked = fake.expect("textDocument/definition");
    fake.write_errors("thread 'main' panicked at src/lib.rs:1:1\n");
    fake.exit(Some(101));

    let error = pending.wait().unwrap_err();
    assert!(matches!(error, RequestError::Exited));
    assert_eq!(error.to_string(), "the server ended before it answered");
    assert_eq!(
        next_event(&events),
        Event::Exited(Exit {
            code: Some(101),
            stderr_tail: "thread 'main' panicked at src/lib.rs:1:1\n".into()
        })
    );
}

#[test]
fn the_exit_keeps_only_the_end_of_a_long_error_output() {
    let (_server, events, mut fake) = initialized(json!({}));
    fake.write_errors(&"x".repeat(20_000));
    fake.write_errors("the last line\n");
    fake.exit(None);
    let Event::Exited(exit) = next_event(&events) else {
        panic!("no exit")
    };
    assert_eq!(exit.code, None);
    assert_eq!(exit.stderr_tail.len(), 8 * 1024);
    assert!(exit.stderr_tail.ends_with("xxthe last line\n"));
}

#[test]
fn a_message_cut_short_ends_the_server_s_output() {
    let (server, events, mut fake) = initialized(json!({}));
    let pending = server.shutdown();
    let _asked = fake.expect("shutdown");
    fake.send_raw(b"Content-Length: 50\r\n\r\n{\"id\":");
    fake.exit(Some(1));
    assert!(matches!(pending.wait(), Err(RequestError::Exited)));
    assert!(matches!(
        next_event(&events),
        Event::Exited(Exit { code: Some(1), .. })
    ));
}

#[test]
fn a_server_that_stops_reading_fails_what_waits_and_what_comes_after() {
    let (server, _events, mut fake) = initialized(json!({}));
    fake.stop_reading();
    assert!(matches!(
        server.shutdown().wait(),
        Err(RequestError::Exited)
    ));
    assert!(matches!(
        somewhere(&server).wait(),
        Err(RequestError::Exited)
    ));
}

#[test]
fn an_answer_that_doesn_t_parse_fails_its_request_alone() {
    let (server, _events, mut fake) = initialized(json!({}));
    let error = definition(&server, &mut fake, json!(42)).unwrap_err();
    assert!(matches!(error, RequestError::Unparsable(_)));
    assert!(
        error
            .to_string()
            .starts_with("the server's answer doesn't parse: "),
        "{error}"
    );

    let pending = somewhere(&server);
    let asked = fake.expect("textDocument/definition");
    fake.send_raw(b"Content-Length: 9\r\n\r\nnot json!");
    fake.send(&json!({"jsonrpc": "2.0"}));
    fake.send(&json!({"jsonrpc": "2.0", "id": asked["id"], "error": {"code": -32603}}));
    let error = pending.wait().unwrap_err();
    assert!(
        error.to_string().contains("a reply of the wrong shape"),
        "{error}"
    );

    assert!(
        definition(&server, &mut fake, Value::Null)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn progress_and_diagnostics_become_events_and_other_notifications_don_t() {
    let (_server, events, mut fake) = initialized(json!({}));
    fake.send(&json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "hi"}}));
    fake.send(&json!({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": 1, "value": {"kind": "nonsense"}}}));
    fake.send(&json!({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "index", "value": {"kind": "end"}}}));
    fake.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": "file:///w/a.c", "diagnostics": []},
    }));
    assert_eq!(
        next_event(&events),
        Event::Progress(Progress {
            token: ProgressToken::Text("index".into()),
            value: WorkDone::End { message: None },
        })
    );
    assert_eq!(
        next_event(&events),
        Event::Diagnostics(Diagnostics {
            uri: uri("file:///w/a.c"),
            version: None,
            diagnostics: Vec::new()
        })
    );
}

#[test]
fn dropping_the_server_kills_it_and_fails_what_waits() {
    let (server, _events, mut fake) = initialized(json!({}));
    // A definition, as in the cancel test, so one instance of `wait_timeout` runs each of its arms.
    let pending = somewhere(&server);
    let _asked = fake.expect("textDocument/definition");
    drop(server);
    fake.killed.recv_timeout(PATIENCE).unwrap();
    assert!(matches!(
        pending.wait_timeout(PATIENCE),
        Some(Err(RequestError::Exited))
    ));
}
