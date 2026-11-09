use super::*;

/// `security.rs` looks for hosts other than GitHub's in these sources, so this one is written apart.
const ELSEWHERE: &str = concat!("https:", "//docs.python.org/3/");

#[test]
fn a_web_page_gets_through() {
    assert_eq!(WebAddress::parse(ELSEWHERE).unwrap().as_str(), ELSEWHERE);
}

#[test]
fn anything_else_is_refused() {
    for text in [
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "vscode://file/etc/passwd",
        "mailto:someone@localhost",
        "/relative/path",
        "#a-heading",
        "",
        concat!("https:", "//someone:secret@docs.python.org/"),
        concat!("https:", "//:github.com@docs.python.org/"),
    ] {
        let problem = WebAddress::parse(text).unwrap_err();
        assert_eq!(problem.kind, ErrorKind::Internal, "{text}");
    }
}

#[test]
fn an_item_has_its_page_on_github() {
    let cpython = RepositoryName::parse("python/cpython").unwrap();
    let page = |item| WebAddress::of_item(&cpython, item);

    assert_eq!(
        page(GitHubItem::Issue { number: 7 }).as_str(),
        "https://github.com/python/cpython/issues/7"
    );
    assert_eq!(
        page(GitHubItem::PullRequest { number: 8 }).as_str(),
        "https://github.com/python/cpython/pull/8"
    );
}
