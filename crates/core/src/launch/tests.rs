use super::*;

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

#[test]
fn a_scheme_counts_whatever_its_case() {
    let text = "TSUGIKI://pr/python/cpython/140124";
    assert_eq!(
        Request::parse(Build::Release, text),
        Request::Link(text.to_owned())
    );
}

#[test]
fn a_link_in_the_other_builds_scheme_is_only_an_argument() {
    let dev = "tsugiki-dev://pr/python/cpython/140124";
    let release = "tsugiki://pr/python/cpython/140124";
    assert_eq!(
        Request::parse(Build::Release, dev),
        Request::Argument(dev.to_owned())
    );
    assert_eq!(
        Request::parse(Build::Development, release),
        Request::Argument(release.to_owned())
    );
}

#[test]
fn anything_else_is_an_argument() {
    for text in ["python/cpython#140124", r"C:\Work\cpython", "tsugiki", ""] {
        assert_eq!(
            Request::parse(Build::Release, text),
            Request::Argument(text.to_owned())
        );
    }
}

#[test]
fn the_program_name_isnt_a_request() {
    let argv = strings(&[
        r"C:\Programs\tsugiki:\tsg.exe",
        "tsugiki://a",
        "python/cpython",
    ]);
    assert_eq!(
        requests(Build::Release, &argv),
        vec![
            Request::Link("tsugiki://a".to_owned()),
            Request::Argument("python/cpython".to_owned()),
        ]
    );
}

#[test]
fn a_program_started_with_nothing_asks_for_nothing() {
    assert_eq!(requests(Build::Release, &strings(&["tsg"])), vec![]);
    assert_eq!(requests(Build::Release, &[]), vec![]);
}

#[test]
fn what_a_request_says_is_one_line_whatever_it_was_given() {
    // Any web page can open a link, so it mustn't be able to write a line of its own in the log.
    let forged = "tsugiki://a\n2026-11-03T00:00:00Z ERROR tsugiki_core: forged\u{1b}[31m";
    assert_eq!(
        Request::parse(Build::Release, forged).to_string(),
        r#"Asked to open the link "tsugiki://a\n2026-11-03T00:00:00Z ERROR tsugiki_core: forged\u{1b}[31m""#
    );
    assert_eq!(
        Request::parse(Build::Release, "a\nb").to_string(),
        r#"Asked to open the argument "a\nb""#
    );
}

#[test]
fn this_programs_command_line_starts_with_its_name() {
    assert!(!command_line().is_empty());
}

#[test]
fn opening_a_command_line_opens_each_request() {
    // Until Alerts arrive opening only logs, and the real-window check reads that line.
    open_command_line(Build::Release, &strings(&["tsg", "tsugiki://a", "b"]));
}

#[test]
fn opening_one_link_needs_no_command_line() {
    open(Build::Release, "tsugiki://a");
}
