use super::*;

fn name(text: &str) -> RepositoryName {
    RepositoryName::parse(text).unwrap()
}

#[test]
fn a_name_is_read_from_what_a_user_types_or_pastes() {
    for text in [
        "python/cpython",
        "  python/cpython\n",
        "python/cpython/",
        "python/cpython.git",
        "github.com/python/cpython",
        "https://github.com/python/cpython",
        "https://github.com/python/cpython/",
        "https://github.com/python/cpython.git",
    ] {
        let parsed = name(text);
        assert_eq!(parsed.owner(), "python", "{text}");
        assert_eq!(parsed.name(), "cpython", "{text}");
    }
}

#[test]
fn a_name_can_hold_what_github_allows() {
    assert_eq!(
        name("some-maintainer/pep848-sim").to_string(),
        "some-maintainer/pep848-sim"
    );
    assert_eq!(name("a/b_c.d-e").to_string(), "a/b_c.d-e");
}

#[test]
fn anything_else_is_refused_before_it_reaches_github() {
    let long_owner = format!("{}/name", "o".repeat(40));
    let long_name = format!("owner/{}", "n".repeat(101));
    for text in [
        "",
        "python",
        "/cpython",
        "python/",
        "python/cpython/issues",
        "python/..",
        "python/.",
        "py thon/cpython",
        "python/cpy thon",
        "py_thon/cpython",
        "python/\"cpython\"",
        "example.com/python/cpython",
        "http://github.com/python/cpython",
        &long_owner,
        &long_name,
    ] {
        let problem = RepositoryName::parse(text).unwrap_err();
        assert_eq!(problem.kind, ErrorKind::NotFound, "{text}");
        assert_eq!(problem.message, BAD_NAME, "{text}");
    }
}

#[test]
fn the_longest_names_github_allows_are_kept() {
    let longest = format!("{}/{}", "o".repeat(39), "n".repeat(100));
    assert_eq!(name(&longest).to_string(), longest);
}

#[test]
fn case_doesnt_make_another_repository() {
    assert!(name("Python/CPython").is_same(&name("python/cpython")));
    assert!(!name("python/cpython").is_same(&name("python/peps")));
}

#[test]
fn a_name_travels_as_a_plain_string_and_is_checked_on_the_way_in() {
    assert_eq!(
        serde_json::to_value(name("python/cpython")).unwrap(),
        "python/cpython"
    );
    assert_eq!(
        serde_json::from_str::<RepositoryName>(r#""python/cpython""#).unwrap(),
        name("python/cpython")
    );
    let problem = serde_json::from_str::<RepositoryName>(r#""nonsense""#).unwrap_err();
    assert!(problem.to_string().contains(BAD_NAME), "{problem}");
}
