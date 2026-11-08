use super::*;

#[test]
fn a_release_answers_to_the_names_users_see() {
    assert_eq!(Build::Release.identifier(), "io.github.tsugiki");
    assert_eq!(Build::Release.scheme(), "tsugiki");
    assert_eq!(Build::Release.folder_name(), "tsugiki");
}

#[test]
fn a_development_build_shares_none_of_them() {
    assert_eq!(Build::Development.identifier(), "io.github.tsugiki.dev");
    assert_eq!(Build::Development.scheme(), "tsugiki-dev");
    assert_eq!(Build::Development.folder_name(), "tsugiki-dev");
}
