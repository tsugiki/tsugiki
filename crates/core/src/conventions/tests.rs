use super::*;

fn for_name(text: &str) -> Conventions {
    Conventions::for_repository(&RepositoryName::parse(text).unwrap())
}

#[test]
fn cpython_gets_its_conventions_and_others_get_githubs() {
    for cpython in ["python/cpython", "Python/CPython"] {
        assert_eq!(for_name(cpython), Conventions::Cpython);
    }
    for other in ["some-maintainer/gcmon", "python/peps", "someone/cpython"] {
        assert_eq!(for_name(other), Conventions::Github);
    }
}

#[test]
fn a_cpython_title_names_its_issues_with_its_prefix() {
    for (title, expected) in [
        ("gh-123: Fix it", vec![123]),
        ("GH-123: Fix it", vec![123]),
        ("[3.14] gh-123: Fix it (GH-456)", vec![123]),
        ("gh-123, gh-456: Fix both", vec![123, 456]),
        ("gh-123 gh-456: Fix both", vec![123, 456]),
        ("[3.14] gh-123, GH-456: Fix both", vec![123, 456]),
        ("gh-123, see gh-456: Fix it", vec![]),
        (": Fix it", vec![]),
        ("Fix it", vec![]),
        ("Fix: gh-123 again", vec![]),
        ("see gh-123: it broke", vec![]),
        ("gh-: Fix it", vec![]),
        ("gh-12x: Fix it", vec![]),
        ("gé: Fix it", vec![]),
        ("[3.14 gh-123: Fix it", vec![]),
    ] {
        assert_eq!(
            Conventions::Cpython.issues_in_title(title),
            expected,
            "{title}"
        );
    }
}

#[test]
fn a_title_could_name_what_any_conventions_read_in_it() {
    assert_eq!(
        Conventions::issues_in_title_under_any("gh-123: Fix it"),
        [123]
    );
}

#[test]
fn a_cpython_title_names_its_origin_pr_with_its_marker() {
    for (title, expected) in [
        ("[3.14] gh-1: Fix it (GH-9)", Some(9)),
        ("[3.14] Doc: Fix it (gh-9)", Some(9)),
        ("[3.14] gh-1: Fix it (GH-9) and more", Some(9)),
        ("[3.14] gh-1: Revert (GH-8) (GH-9)", Some(9)),
        ("[3.14] gh-1: Fix it (GH-9) (see notes)", Some(9)),
        ("[3.14] gh-1: Fix it", None),
        ("[3.14] gh-1: Fix it GH-9", None),
        ("[3.14] gh-1: Fix it (#9)", None),
        ("[3.14] gh-1: Fix it (GH-)", None),
        ("[3.14] gh-1: Fix it (GH-9", None),
        ("Fix it", None),
    ] {
        assert_eq!(
            Conventions::Cpython.origin_in_title(title),
            expected,
            "{title}"
        );
    }
}

#[test]
fn githubs_conventions_have_no_backports() {
    let title = "[3.14] gh-1: Fix it (GH-9)";
    assert_eq!(Conventions::Github.origin_in_title(title), None);
    assert!(!Conventions::Github.has_backports());
    assert!(Conventions::Cpython.has_backports());
}

#[test]
fn a_title_could_name_the_origin_any_conventions_read_in_it() {
    assert_eq!(
        Conventions::origin_in_title_under_any("[3.14] gh-1: Fix it (GH-9)"),
        Some(9)
    );
    assert_eq!(Conventions::origin_in_title_under_any("Fix it"), None);
}

#[test]
fn cpython_knows_its_bot_accounts_and_github_knows_none() {
    for bot in ["miss-islington", "bedevere-bot"] {
        assert!(
            Conventions::Cpython.is_bot_account(&Login::new(bot)),
            "{bot}"
        );
        assert!(
            !Conventions::Github.is_bot_account(&Login::new(bot)),
            "{bot}"
        );
    }
    assert!(!Conventions::Cpython.is_bot_account(&Login::new("some-author")));
}

#[test]
fn the_backport_bots_words_name_the_branch_it_failed_to_reach() {
    let app = Login::new("miss-islington-app");
    let target = |comment: &str| Conventions::Cpython.failed_backport_target(&app, comment);
    assert_eq!(
        target("Sorry, @a and @b, I could not cleanly backport this to 3.14 due to a conflict."),
        Some("3.14".to_owned())
    );
    assert_eq!(
        target("Sorry @a, I had trouble checking out the `3.13` backport branch."),
        Some("3.13".to_owned())
    );
    assert_eq!(
        target("Thanks @a for the PR, I'm working now to backport this."),
        None
    );
    assert_eq!(target("I could not cleanly backport this to 3.14"), None);
}

#[test]
fn only_the_backport_bot_can_say_a_bot_backport_failed() {
    let words = "I could not cleanly backport this to 3.14 due to a conflict.";
    let person = Login::new("some-author");
    assert_eq!(
        Conventions::Cpython.failed_backport_target(&person, words),
        None
    );
    let app = Login::new("miss-islington-app");
    assert_eq!(
        Conventions::Github.failed_backport_target(&app, words),
        None
    );
}

#[test]
fn cpython_has_a_backport_bot_and_github_has_none() {
    let bot = Login::new("Miss-Islington");
    assert!(Conventions::Cpython.is_backport_bot(&bot));
    assert!(!Conventions::Cpython.is_backport_bot(&Login::new("some-author")));
    assert!(!Conventions::Github.is_backport_bot(&bot));
}
