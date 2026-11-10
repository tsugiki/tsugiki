use super::*;
use crate::error::ErrorKind;
use std::io;

fn gh_prints(stdout: &str) -> io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: stdout.to_owned(),
    })
}

#[test]
fn the_token_is_what_gh_printed() {
    let token = read_token(|_| gh_prints("gho_example\n")).unwrap();
    assert_eq!(token.hide_in("Bearer gho_example", "***"), "Bearer ***");
}

#[test]
fn without_gh_tsugiki_says_how_to_sign_in() {
    let error = read_token(|_| Err(io::Error::from(io::ErrorKind::NotFound))).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert!(error.message.contains("gh auth login"), "{}", error.message);
}

#[test]
fn when_gh_is_signed_out_tsugiki_says_how_to_sign_in() {
    let error = read_token(|_| {
        Ok(GhOutput {
            success: false,
            stdout: String::new(),
        })
    })
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert!(error.message.contains("gh auth login"), "{}", error.message);
}

#[test]
fn an_empty_answer_from_gh_is_not_a_token() {
    let error = read_token(|_| gh_prints(" \n")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
}
