use super::*;
use crate::error::ErrorKind;
use tsugiki_token::testing::{NO_GH, SLOW_GH, gh_alone, gh_prints};

#[test]
fn the_token_is_what_gh_printed() {
    let token = read_token(gh_alone(|_| gh_prints("gho_example\n"))).unwrap();
    assert_eq!(token.hide_in("Bearer gho_example", "***"), "Bearer ***");
}

#[test]
fn without_gh_tsugiki_says_how_to_sign_in() {
    let error = read_token(NO_GH).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(error.message, SIGN_IN);
}

#[test]
fn when_gh_doesnt_answer_in_time_tsugiki_says_so_and_to_restart() {
    let error = read_token(SLOW_GH).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(
        error.message,
        "Tsugiki reads your token from the GitHub CLI, and `gh` didn't answer within 30 s. Restart \
         Tsugiki to ask it again."
    );
}

#[test]
fn when_gh_is_signed_out_tsugiki_says_how_to_sign_in() {
    let error = read_token(gh_alone(|_| {
        Ok(GhOutput {
            success: false,
            stdout: String::new(),
        })
    }))
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert!(error.message.contains("gh auth login"), "{}", error.message);
}

#[test]
fn an_empty_answer_from_gh_is_not_a_token() {
    let error = read_token(gh_alone(|_| gh_prints(" \n"))).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
}
