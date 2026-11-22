#![expect(
    clippy::disallowed_methods,
    reason = "these are the tests of `Token::find` itself"
)]

use std::time::Duration;

use rustls::pki_types::UnixTime;

use super::*;
use crate::testing::{NO_GH, SLOW_GH, gh_alone, gh_prints, nothing_set, refusal};

fn example() -> Token {
    Token::find(gh_alone(|_| gh_prints("gho_example\n"))).unwrap()
}

const GH_NEVER_RUNS: RunGh = |_| panic!("gh ran although GH_TOKEN holds a token");

#[test]
fn the_token_comes_from_gh_auth_token_for_github_com() {
    let token = Token::find(gh_alone(|args| {
        assert_eq!(args, ["auth", "token", "--hostname", "github.com"]);
        gh_prints("gho_example\n")
    }))
    .unwrap();
    assert_eq!(token.bearer(), "Bearer gho_example");
}

#[test]
fn gh_token_set_gives_its_token_and_runs_no_gh() {
    let token = Token::find(TokenFrom {
        env: || Some("ghp_from_env\n".to_owned()),
        gh: GH_NEVER_RUNS,
    })
    .unwrap();
    assert_eq!(token.bearer(), "Bearer ghp_from_env");
}

#[test]
fn without_gh_token_or_with_it_empty_the_token_comes_from_gh() {
    let unset_or_empty: [ReadEnv; 3] = [
        nothing_set,
        || Some(String::new()),
        || Some(" \n".to_owned()),
    ];
    for env in unset_or_empty {
        let token = Token::find(TokenFrom {
            env,
            gh: |_| gh_prints("gho_example\n"),
        })
        .unwrap();
        assert_eq!(token.bearer(), "Bearer gho_example");
    }
}

#[test]
fn a_token_from_gh_token_is_hidden_wherever_it_appears() {
    let token = Token::find(TokenFrom {
        env: || Some("0123456789abcdef".to_owned()),
        gh: GH_NEVER_RUNS,
    })
    .unwrap();
    assert_eq!(
        token.hide_in("Bearer 0123456789abcdef refused", "[token]"),
        "Bearer [token] refused"
    );
}

#[test]
fn gh_that_cant_run_gives_no_token_and_says_why() {
    let missing = Token::find(NO_GH);
    assert!(
        matches!(missing, Err(NoToken::GhMissing(cause)) if cause.kind() == io::ErrorKind::NotFound)
    );
}

#[test]
fn gh_that_doesnt_answer_in_time_gives_no_token_and_says_so() {
    let slow = Token::find(SLOW_GH);
    assert!(matches!(slow, Err(NoToken::GhTimedOut)), "{slow:?}");
}

#[test]
fn gh_that_fails_gives_no_token() {
    let failed = Token::find(gh_alone(|_| {
        Ok(GhOutput {
            success: false,
            stdout: "gho_example".to_owned(),
        })
    }));
    assert!(matches!(failed, Err(NoToken::GhFailed)));
}

#[test]
fn an_empty_answer_from_gh_is_not_a_token() {
    let empty = Token::find(gh_alone(|_| gh_prints(" \n")));
    assert!(matches!(empty, Err(NoToken::GhPrintedNothing)));
}

#[test]
fn printing_a_token_doesnt_show_it() {
    let token = example();
    assert_eq!(format!("{token:?}"), "Token(***)");
    assert_eq!(format!("{:?}", token.clone()), "Token(***)");
}

#[test]
fn a_token_is_hidden_wherever_it_appears() {
    let token = Token::find(gh_alone(|_| gh_prints("0123456789abcdef"))).unwrap();
    assert_eq!(
        token.hide_in("Bearer 0123456789abcdef, again 0123456789abcdef", "[token]"),
        "Bearer [token], again [token]"
    );
    assert_eq!(token.hide_in("nothing secret", "[token]"), "nothing secret");
}

#[test]
fn text_shaped_like_any_github_token_is_hidden() {
    let text = "gho_abc123 ghp_ABC ghs_a_b ghu_1 ghr_2 github_pat_11A_b0, other";
    assert_eq!(
        hide_token_shapes(text, "[token]"),
        "[token] [token] [token] [token] [token] [token], other"
    );
}

#[test]
fn words_that_only_start_like_a_token_stay() {
    assert_eq!(hide_token_shapes("gho_ ghost", "[token]"), "gho_ ghost");
}

#[test]
fn a_token_right_after_an_escape_is_hidden() {
    assert_eq!(
        hide_token_shapes(r"see:\ngho_abc %3Dghp_def", "[token]"),
        r"see:\n[token] %3D[token]"
    );
}

#[test]
fn other_text_stays_including_non_ascii() {
    assert_eq!(hide_token_shapes("tsugi木 ok", "[token]"), "tsugi木 ok");
}

#[test]
fn a_rest_path_starts_with_a_slash() {
    let path = RestPath::parse("/repos/python/cpython/issues").unwrap();
    assert_eq!(path.to_string(), "/repos/python/cpython/issues");
    assert_eq!(RestPath::parse("repos/python/cpython"), None);
    assert_eq!(RestPath::parse(""), None);
}

#[test]
fn a_rest_path_refuses_what_could_name_another_host() {
    for text in [
        "/repos@evil.example/x",
        "/repos?next=https://evil.example",
        "/repos#evil.example",
        "/\\evil.example/x",
    ] {
        assert_eq!(RestPath::parse(text), None, "{text}");
    }
}

#[test]
fn a_rest_path_refuses_segments_that_move_it_elsewhere() {
    for text in [
        "/repos/a/b/../../user",
        "/repos/./x",
        "/repos/%2e%2E/user",
        "/repos/.%2e",
        "/repos/..",
    ] {
        assert_eq!(RestPath::parse(text), None, "{text}");
    }
    assert!(RestPath::parse("/repos/python/.github").is_some());
    assert!(RestPath::parse("/repos/a/b...c").is_some());
}

#[test]
fn a_query_is_encoded_so_it_stays_a_query() {
    let path = RestPath::parse("/repos/python/cpython/issues")
        .unwrap()
        .with_query(&[("state", "all"), ("q", "a&b=@#/")]);
    assert_eq!(
        path.to_string(),
        "/repos/python/cpython/issues?state=all&q=a%26b%3D%40%23%2F"
    );
    assert_eq!(
        path.url().unwrap().as_str(),
        "https://api.github.com/repos/python/cpython/issues?state=all&q=a%26b%3D%40%23%2F"
    );
}

#[test]
fn a_path_that_looks_like_a_host_stays_on_github() {
    let url = RestPath::parse("//evil.example/x").unwrap().url().unwrap();
    assert_eq!(url.host_str(), Some("api.github.com"));
}

#[test]
fn only_api_github_com_over_https_counts_as_github() {
    assert!(is_github(&Url::parse(GRAPHQL).unwrap()));
    assert!(is_github(&Url::parse(REST_ROOT).unwrap()));
    for other in [
        "http://api.github.com/graphql",
        "https://evil.example/graphql",
        "https://api.github.com.evil.example/",
        "https://api.github.com:8443/",
        "https://someone@api.github.com/",
        "https://:secret@api.github.com/",
    ] {
        assert!(!is_github(&Url::parse(other).unwrap()), "{other}");
    }
}

/// A handshake that rustls failed, as ureq hands it back.
fn handshake_failed(error: CertificateError) -> ureq::Error {
    let tls = rustls::Error::InvalidCertificate(error);
    ureq::Error::Io(io::Error::new(io::ErrorKind::InvalidData, tls))
}

#[test]
fn a_certificate_outside_its_dates_is_about_the_clock() {
    // webpki's verifier, which ureq uses, gives the two with context.
    let time = UnixTime::since_unix_epoch(Duration::from_secs(1_800_000_000));
    let causes = [
        handshake_failed(CertificateError::ExpiredContext {
            time,
            not_after: UnixTime::since_unix_epoch(Duration::from_secs(1_700_000_000)),
        }),
        handshake_failed(CertificateError::NotValidYetContext {
            time,
            not_before: UnixTime::since_unix_epoch(Duration::from_secs(1_900_000_000)),
        }),
        handshake_failed(CertificateError::Expired),
        handshake_failed(CertificateError::NotValidYet),
    ];
    for cause in causes {
        assert_eq!(
            unreachable(&cause),
            Unreachable::CertificateDates(cause.to_string())
        );
    }
}

#[test]
fn any_other_failure_is_offline() {
    let causes = [
        handshake_failed(CertificateError::UnknownIssuer),
        handshake_failed(CertificateError::ExpiredRevocationList),
        ureq::Error::Rustls(rustls::Error::InvalidCertificate(CertificateError::Expired)),
        ureq::Error::Io(io::Error::new(io::ErrorKind::TimedOut, "timed out")),
        ureq::Error::HostNotFound,
    ];
    for cause in causes {
        assert_eq!(unreachable(&cause), Unreachable::Offline(cause.to_string()));
    }
}

/// A GraphQL body of `query` with no variables.
fn body_of(query: &str) -> String {
    serde_json::json!({ "query": query, "variables": null }).to_string()
}

#[test]
fn a_read_goes_out_as_the_crate_writes_it_again() {
    let body = r#"{ "variables" : { "n" : 3 }, "query" : "query Op($n: Int!) { a(n: $n) }" }"#;
    assert_eq!(
        checked_read(body).unwrap(),
        r#"{"query":"query Op($n: Int!) { a(n: $n) }","variables":{"n":3}}"#
    );
}

#[test]
fn a_query_that_holds_mutation_anywhere_is_refused() {
    let write = "mutation { deleteRef(input: { refId: \"R\" }) { clientMutationId } }";
    let queries = [
        write.to_owned(),
        format!("query A {{ viewer {{ login }} }} {write}"),
        "query { viewer { login } # mutation\n }".to_owned(),
        "query { search(query: \"mutation\", type: ISSUE, first: 1) { issueCount } }".to_owned(),
        "query { mutations: viewer { login } }".to_owned(),
    ];
    for query in queries {
        assert_eq!(
            checked_read(&body_of(&query)),
            Err(NotARead::Mutation),
            "{query}"
        );
    }
}

#[test]
fn a_body_with_another_key_or_a_key_twice_is_refused() {
    let read = "query { viewer { login } }";
    let bodies = [
        format!(r#"{{"query":"{read}","variables":null,"operationName":"Op"}}"#),
        // Variables written as `null, "query": "mutation …"` by a builder that pastes them in.
        format!(r#"{{"query":"{read}","variables":null, "query": "mutation {{ a }}"}}"#),
        format!(r#"{{"query":"{read}","variables":null,"variables":{{}}}}"#),
        format!(r#"{{"query":"{read}","variables":null}}{{"query":"mutation {{ a }}"}}"#),
        format!(r#"{{"query":"{read}"}}"#),
        "[]".to_owned(),
    ];
    for body in bodies {
        assert!(
            matches!(checked_read(&body), Err(NotARead::Unparsed(_))),
            "{body}"
        );
    }
}

#[test]
fn a_key_twice_inside_the_variables_goes_out_once() {
    let body = r#"{"query":"query Op($n: Int!) { a(n: $n) }","variables":{"n":1,"n":2}}"#;
    let sent = checked_read(body).unwrap();
    assert_eq!(sent.matches(r#""n""#).count(), 1, "{sent}");
    assert!(sent.contains(r#""n":2"#), "{sent}");
}

#[test]
fn a_refusal_says_why() {
    let mutation = refusal(&body_of("mutation { a }")).unwrap();
    assert!(mutation.contains("`mutation`"), "{mutation}");
    let unparsed = refusal(r#"{"query":"","variables":null,"extra":1}"#).unwrap();
    assert!(unparsed.contains("unknown field `extra`"), "{unparsed}");
    assert_eq!(refusal(&body_of("query { viewer { login } }")), None);
}
