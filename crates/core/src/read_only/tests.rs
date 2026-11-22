use super::*;

#[test]
fn a_writable_repository_online_has_no_reason_to_be_read_only() {
    let state = RepositoryState::new([], Connection::Online);
    assert_eq!(state.read_only, []);
    assert_eq!(state.connection, Connection::Online);
}

#[test]
fn a_repository_the_token_cant_write_to_is_read_only_for_the_token() {
    let state = RepositoryState::new([ReadOnly::Token], Connection::Online);
    assert_eq!(state.read_only, [ReadOnly::Token]);
    assert_eq!(state.connection, Connection::Online);
}

#[test]
fn offline_a_writable_repository_stays_writable_and_says_offline() {
    let state = RepositoryState::new([], Connection::Offline);
    assert_eq!(state.read_only, []);
    assert_eq!(state.connection, Connection::Offline);
}

#[test]
fn a_reason_given_twice_counts_once() {
    let state = RepositoryState::new([ReadOnly::Token, ReadOnly::Token], Connection::Online);
    assert_eq!(state.read_only, [ReadOnly::Token]);
}

#[test]
fn the_state_goes_to_the_window_as_typed_values() {
    let state = RepositoryState::new([ReadOnly::Token], Connection::Offline);
    assert_eq!(
        serde_json::to_value(state).unwrap(),
        serde_json::json!({ "read_only": ["token"], "connection": "offline" })
    );
}

#[test]
fn the_users_choice_comes_before_the_token_and_each_reaches_the_window_by_name() {
    let state = RepositoryState::new([ReadOnly::Token, ReadOnly::Chosen], Connection::Online);
    assert_eq!(state.read_only, [ReadOnly::Chosen, ReadOnly::Token]);
    assert_eq!(
        serde_json::to_value(state).unwrap()["read_only"],
        serde_json::json!(["chosen", "token"])
    );
}

#[test]
fn a_repository_no_reason_holds_is_writable() {
    let writable = WritableRepository::new(&crate::testing::cpython(), []).unwrap();
    assert_eq!(writable.repository(), &crate::testing::cpython());
}

#[test]
fn a_repository_a_reason_holds_gives_the_first_reason_and_isnt_writable() {
    let cpython = crate::testing::cpython();
    let refused = |reasons: &[ReadOnly]| WritableRepository::new(&cpython, reasons.to_vec());
    assert_eq!(refused(&[ReadOnly::Chosen]).unwrap_err(), ReadOnly::Chosen);
    assert_eq!(refused(&[ReadOnly::Token]).unwrap_err(), ReadOnly::Token);
    let both = refused(&[ReadOnly::Token, ReadOnly::Chosen]);
    assert_eq!(both.unwrap_err(), ReadOnly::Chosen);
}

#[test]
fn repo_writes_everywhere_and_no_scope_nowhere_whatever_the_visibility() {
    for visibility in [None, Some(Visibility::Public), Some(Visibility::Private)] {
        assert_eq!(token_cant_write(WriteScope::Every, visibility), Some(false));
        assert_eq!(token_cant_write(WriteScope::None, visibility), Some(true));
    }
}

#[test]
fn public_repo_alone_cant_write_to_a_private_repository_and_waits_to_know_which() {
    let public = WriteScope::Public;
    assert_eq!(
        token_cant_write(public, Some(Visibility::Public)),
        Some(false)
    );
    assert_eq!(
        token_cant_write(public, Some(Visibility::Private)),
        Some(true)
    );
    assert_eq!(token_cant_write(public, None), None);
}
