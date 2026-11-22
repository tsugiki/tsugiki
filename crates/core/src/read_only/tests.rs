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
