use super::*;
use crate::error::ErrorKind;
use tsugiki_token::testing::{NO_GH, SLOW_GH, gh_alone, gh_prints};

const BOTH: [AskAgain; 2] = [AskAgain::AtRestart, AskAgain::OnRetry];

#[test]
fn the_token_is_what_gh_printed() {
    let token = read_token(
        gh_alone(|_| gh_prints("gho_example\n")),
        AskAgain::AtRestart,
    )
    .unwrap();
    assert_eq!(token.hide_in("Bearer gho_example", "***"), "Bearer ***");
}

#[test]
fn without_gh_tsugiki_says_how_to_sign_in() {
    for again in BOTH {
        let error = read_token(NO_GH, again).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert_eq!(error.message, SIGN_IN);
    }
}

#[test]
fn when_gh_doesnt_answer_in_time_at_start_tsugiki_says_so_and_to_restart() {
    let error = read_token(SLOW_GH, AskAgain::AtRestart).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(
        error.message,
        "Tsugiki reads your token from the GitHub CLI, and `gh` didn't answer within 30 s. Restart \
         Tsugiki to ask it again."
    );
}

#[test]
fn when_gh_doesnt_answer_in_time_for_a_retry_tsugiki_says_it_asks_again() {
    let error = read_token(SLOW_GH, AskAgain::OnRetry).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(
        error.message,
        "Tsugiki reads your token from the GitHub CLI, and `gh` didn't answer within 30 s. Tsugiki \
         asks it again on the next try."
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
    for again in BOTH {
        let error = read_token(signed_out, again).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert_eq!(error.message, SIGN_IN);
    }
}

#[test]
fn an_empty_answer_from_gh_is_not_a_token() {
    let error = read_token(gh_alone(|_| gh_prints(" \n")), AskAgain::OnRetry).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert_eq!(error.message, SIGN_IN);
}
