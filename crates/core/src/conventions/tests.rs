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
fn only_cpythons_conventions_have_backports() {
    assert!(!Conventions::Github.has_backports());
    assert!(Conventions::Cpython.has_backports());
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
