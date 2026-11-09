use super::*;
use crate::repository::RepositoryCounts;

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

/// The check of a Repository named `text` that GitHub counted so, for a user who doesn't administer it.
fn checked(text: &str, counts: RepositoryCounts) -> RepositoryCheck {
    RepositoryCheck::new(FoundRepository {
        name: name(text),
        counts,
        access: RepositoryAccess::Other,
    })
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
    let check = checked("some-maintainer/gcmon", FEW);
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
    let check = checked("a/b", at_the_limit);
    assert_eq!(check.size, RepositorySize::Small);
    assert_eq!(check.first_sync_items, 535);
}

#[test]
fn a_repository_the_user_administers_starts_with_the_filters_for_its_maintainers() {
    let check = RepositoryCheck::new(FoundRepository {
        name: name("a/b"),
        counts: FEW,
        access: RepositoryAccess::Maintained,
    });
    assert_eq!(
        check.repository.filters,
        Filter::starters(RepositoryAccess::Maintained)
    );
}

#[test]
fn above_500_open_is_large_and_loads_the_window() {
    let just_over = RepositoryCounts {
        open_issues: 401,
        open_prs: 100,
        ..FEW
    };
    let check = checked("a/b", just_over);
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
    let check = checked("python/cpython", cpython);
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
    let check = checked("a/b", huge);
    assert_eq!(check.mirror_mb, u32::MAX);
}
