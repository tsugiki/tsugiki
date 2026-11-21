use super::*;
use crate::error::ErrorKind;
use tsugiki_token::testing::{NO_GH, SLOW_GH, gh_alone, gh_prints};

const SIGN_IN: &str = "Tsugiki reads your token from the GitHub CLI, and found none. Install `gh` or run \
                       `gh auth login`. Tsugiki asks again each minute, and at once when you click the \
                       sync status. To skip `gh`, set `GH_TOKEN` to a token and restart Tsugiki.";

#[test]
fn the_token_is_what_gh_printed() {
    let token = read_token(gh_alone(|_| gh_prints("gho_example\n"))).unwrap();
    assert_eq!(token.hide_in("Bearer gho_example", "***"), "Bearer ***");
}

#[test]
fn without_gh_tsugiki_says_how_to_sign_in_and_that_it_asks_again() {
    let error = read_token(NO_GH).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(error.message, SIGN_IN);
}

#[test]
fn when_gh_doesnt_answer_in_time_tsugiki_says_so_that_it_asks_again_and_how_to_skip_gh() {
    let error = read_token(SLOW_GH).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(
        error.message,
        "Tsugiki asked `gh auth token` for your token, and `gh` didn't answer within 30 s. A keychain or \
         credential manager waiting on a prompt is a common cause. Tsugiki asks again each minute, and at \
         once when you click the sync status. To skip `gh`, set `GH_TOKEN` to a token and restart Tsugiki."
    );
}

#[test]
fn when_gh_is_signed_out_tsugiki_says_how_to_sign_in() {
    let signed_out = gh_alone(|_| {
        Ok(GhOutput {
            success: false,
            stdout: String::new(),
        })
    });
    let error = read_token(signed_out).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(error.message, SIGN_IN);
}

#[test]
fn an_empty_answer_from_gh_is_not_a_token() {
    let error = read_token(gh_alone(|_| gh_prints(" \n"))).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(error.message, SIGN_IN);
}
