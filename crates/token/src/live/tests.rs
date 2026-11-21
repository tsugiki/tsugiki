use super::*;

/// Set in a copy of this test binary that should sleep through `a_copy_of_this_binary_sleeps_when_asked`.
const SLEEP: &str = "TSUGIKI_TOKEN_TEST_SLEEPS";

/// A copy of this test binary running one test, which sleeps while `env` names `SLEEP`.
fn copy_of_this_binary(env: &[(&str, &str)]) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "live::tests::a_copy_of_this_binary_sleeps_when_asked",
        ])
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_copy_of_this_binary_sleeps_when_asked() {
    if std::env::var_os(SLEEP).is_some() {
        thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn a_program_that_ends_in_time_gives_what_it_printed() {
    let output = wait_for(copy_of_this_binary(&[]), Duration::from_secs(60)).unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}

#[test]
fn a_program_past_its_limit_is_stopped_and_times_out() {
    let started = Instant::now();
    let outcome = wait_for(
        copy_of_this_binary(&[(SLEEP, "1")]),
        Duration::from_millis(200),
    );
    assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
}

fn latest_release() -> RestPath {
    RestPath::parse("/repos/tsugiki/tsugiki/releases/latest").unwrap()
}

#[test]
fn a_public_request_carries_no_authorization_header() {
    let request = Https::new().public_request(&latest_release()).unwrap();
    let headers = request.headers_ref().unwrap();
    assert!(headers.get("authorization").is_none());
    assert_eq!(headers["accept"], "application/vnd.github+json");
}

#[test]
fn a_public_request_goes_to_the_path_on_api_github_com() {
    let request = Https::new().public_request(&latest_release()).unwrap();
    assert_eq!(
        request.uri_ref().unwrap(),
        "https://api.github.com/repos/tsugiki/tsugiki/releases/latest"
    );
}
