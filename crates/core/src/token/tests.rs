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
fn the_token_comes_from_gh_auth_token_for_github_com() {
    let token = Token::from_gh(|args| {
        assert_eq!(args, ["auth", "token", "--hostname", "github.com"]);
        gh_prints("gho_example\n")
    })
    .unwrap();
    assert_eq!(token.expose(), "gho_example");
}

#[test]
fn without_gh_tsugiki_says_how_to_sign_in() {
    let error = Token::from_gh(|_| Err(io::Error::from(io::ErrorKind::NotFound))).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
    assert!(error.message.contains("gh auth login"), "{}", error.message);
}

#[test]
fn when_gh_is_signed_out_tsugiki_says_how_to_sign_in() {
    let error = Token::from_gh(|_| {
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
    let error = Token::from_gh(|_| gh_prints(" \n")).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unauthorized);
}

#[test]
fn printing_a_token_doesnt_show_it() {
    let token = Token::from_gh(|_| gh_prints("gho_example")).unwrap();
    assert_eq!(format!("{token:?}"), "Token(***)");
}
