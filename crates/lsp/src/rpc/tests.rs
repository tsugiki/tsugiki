use serde_json::{Value, json};

use super::{
    Incoming, METHOD_NOT_FOUND, ReplyError, RequestId, ResponseError, Unreadable, notification,
    parse, reply, request,
};

fn parsed(message: &Value) -> Incoming {
    parse(message.to_string().as_bytes()).unwrap()
}

#[test]
fn a_reply_carries_its_result_even_when_that_is_null() {
    assert_eq!(
        parsed(&json!({"jsonrpc": "2.0", "id": 4, "result": null})),
        Incoming::Reply {
            id: RequestId::Number(4),
            outcome: Ok(Value::Null)
        }
    );
    assert_eq!(
        parsed(&json!({"jsonrpc": "2.0", "id": 5, "result": [1]})),
        Incoming::Reply {
            id: RequestId::Number(5),
            outcome: Ok(json!([1]))
        }
    );
}

#[test]
fn a_reply_with_an_error_carries_the_error() {
    assert_eq!(
        parsed(&json!({"id": 6, "error": {"code": -32800, "message": "cancelled", "data": 1}})),
        Incoming::Reply {
            id: RequestId::Number(6),
            outcome: Err(ReplyError::Server(ResponseError {
                code: -32800,
                message: "cancelled".into()
            })),
        }
    );
}

#[test]
fn a_reply_of_the_wrong_shape_still_answers_its_request() {
    for body in [
        json!({"id": 7}),
        json!({"id": 7, "error": {"code": -32603}}),
    ] {
        let Incoming::Reply {
            id: RequestId::Number(7),
            outcome: Err(ReplyError::Malformed(why)),
        } = parsed(&body)
        else {
            panic!("{body}");
        };
        assert!(why.starts_with("a reply of the wrong shape: {"), "{why}");
    }
}

#[test]
fn a_method_with_an_id_is_a_request_and_without_one_a_notification() {
    assert_eq!(
        parsed(&json!({"id": "a1", "method": "workspace/configuration", "params": {"items": []}})),
        Incoming::Request {
            id: RequestId::Text("a1".into()),
            method: "workspace/configuration".into(),
            params: json!({"items": []}),
        }
    );
    assert_eq!(
        parsed(&json!({"method": "$/progress"})),
        Incoming::Notification {
            method: "$/progress".into(),
            params: Value::Null
        }
    );
}

#[test]
fn json_that_isn_t_a_message_and_text_that_isn_t_json_are_unreadable() {
    for body in [
        json!({"result": 1}),
        json!({"error": {"code": 1, "message": ""}}),
        json!({"id": "x", "error": {"code": 1}}),
        json!({"id": 1, "method": 2}),
        json!([1]),
    ] {
        match parse(body.to_string().as_bytes()) {
            Err(error @ Unreadable::NotAMessage(_)) => {
                assert!(error.to_string().starts_with("isn't a JSON-RPC message: "));
            }
            other => panic!("{body}: {other:?}"),
        }
    }
    match parse(b"{not json") {
        Err(error @ Unreadable::Json(_)) => assert!(error.to_string().starts_with("isn't JSON: ")),
        other => panic!("{other:?}"),
    }
}

fn written(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn the_client_writes_requests_notifications_and_replies() {
    assert_eq!(
        written(&request(3, "shutdown", Value::Null)),
        json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"})
    );
    assert_eq!(
        written(&request(4, "textDocument/definition", json!({"a": 1}))),
        json!({"jsonrpc": "2.0", "id": 4, "method": "textDocument/definition", "params": {"a": 1}})
    );
    assert_eq!(
        written(&notification("exit", Value::Null)),
        json!({"jsonrpc": "2.0", "method": "exit"})
    );
    assert_eq!(
        written(&notification("initialized", json!({}))),
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})
    );
    assert_eq!(
        written(&reply(&RequestId::Text("x".into()), Ok(json!([null])))),
        json!({"jsonrpc": "2.0", "id": "x", "result": [null]})
    );
    let refused = ResponseError {
        code: METHOD_NOT_FOUND,
        message: "no".into(),
    };
    assert_eq!(
        written(&reply(&RequestId::Number(9), Err(refused))),
        json!({"jsonrpc": "2.0", "id": 9, "error": {"code": -32601, "message": "no"}})
    );
}
