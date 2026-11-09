use super::*;

fn name(text: &str) -> RepositoryName {
    RepositoryName::parse(text).unwrap()
}

const FEW: RepositoryCounts = RepositoryCounts {
    open_issues: 10,
    open_prs: 3,
    updated_in_window: 43,
    closed_in_window: 35,
    disk_kb: 0,
};

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

#[test]
fn conventions_a_file_leaves_out_come_from_the_name() {
    let read = |text: &str| toml_edit::de::from_str::<RepositorySettings>(text).unwrap();
    assert_eq!(
        read(r#"name = "python/cpython""#).conventions,
        Conventions::Cpython
    );
    assert_eq!(
        read(r#"name = "python/peps""#).conventions,
        Conventions::Github
    );
    assert_eq!(
        read("name = \"python/cpython\"\nconventions = \"github\"").conventions,
        Conventions::Github
    );
}

#[test]
fn up_to_500_open_is_small_and_loads_those_and_what_closed_lately() {
    let check = RepositoryCheck::new(name("some-maintainer/gcmon"), FEW, RepositoryAccess::Other);
    assert_eq!(check.size, RepositorySize::Small);
    assert_eq!(check.open_issues, 10);
    assert_eq!(check.open_prs, 3);
    assert_eq!(check.first_sync_items, 48);
    assert_eq!(check.window_days, 30);

    let at_the_limit = RepositoryCounts {
        open_issues: 400,
        open_prs: 100,
        ..FEW
    };
    let check = RepositoryCheck::new(name("a/b"), at_the_limit, RepositoryAccess::Other);
    assert_eq!(check.size, RepositorySize::Small);
    assert_eq!(check.first_sync_items, 535);
}

#[test]
fn above_500_open_is_large_and_loads_the_window() {
    let just_over = RepositoryCounts {
        open_issues: 401,
        open_prs: 100,
        ..FEW
    };
    let check = RepositoryCheck::new(name("a/b"), just_over, RepositoryAccess::Other);
    assert_eq!(check.size, RepositorySize::Large);
    assert_eq!(check.first_sync_items, 43);
}

#[test]
fn the_estimate_for_cpython_agrees_with_what_was_measured() {
    // A first sync measured about 485 points and 20.5 minutes for 4,760 items.
    let cpython = RepositoryCounts {
        open_issues: 6935,
        open_prs: 2308,
        updated_in_window: 4715,
        closed_in_window: 2506,
        disk_kb: 912_867,
    };
    let check = RepositoryCheck::new(name("python/cpython"), cpython, RepositoryAccess::Other);
    assert_eq!(check.repository.conventions, Conventions::Cpython);
    assert!((460..=510).contains(&check.first_sync_points), "{check:?}");
    assert!(
        (1150..=1300).contains(&check.first_sync_seconds),
        "{check:?}"
    );
    // Its mirror took about 3.5 minutes from GitHub alone, and about 36 s borrowing.
    assert_eq!(check.mirror_mb, 891);
    assert!((200..=220).contains(&check.download_seconds), "{check:?}");
    assert!((33..=39).contains(&check.borrow_seconds), "{check:?}");
}

#[test]
fn a_size_past_what_the_window_can_count_says_the_most_it_can() {
    let huge = RepositoryCounts {
        disk_kb: u64::MAX,
        ..FEW
    };
    let check = RepositoryCheck::new(name("a/b"), huge, RepositoryAccess::Other);
    assert_eq!(check.mirror_mb, u32::MAX);
}
