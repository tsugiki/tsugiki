use super::*;

#[test]
fn a_login_is_the_same_user_whatever_its_case() {
    assert_eq!(Login::new("Octocat"), Login::new("octocat"));
    assert_ne!(Login::new("octocat"), Login::new("octodog"));
}

#[test]
fn a_login_keeps_githubs_casing_for_display() {
    assert_eq!(Login::new("Octocat").as_str(), "Octocat");
}

#[test]
fn a_login_reads_a_name_that_names_one_person() {
    assert_eq!(
        Login::parse(" Octocat ").map(|login| login.as_str().to_owned()),
        Some("Octocat".to_owned())
    );
    assert_eq!(Login::parse("two words"), None);
    assert_eq!(Login::parse("  "), None);
}
