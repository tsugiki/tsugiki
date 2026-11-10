use super::*;

const NUMBER: &str = env!("CARGO_PKG_VERSION");

#[test]
fn a_build_names_its_commit_as_github_shows_it() {
    let version = Version::new(Some("3A6DC89f0e1d2c3b4a5968778695a4b3c2d1e0f9"));
    assert_eq!(version.to_string(), format!("{NUMBER} (3a6dc89)"));
}

#[test]
fn a_build_with_no_commit_gives_the_version_alone() {
    assert_eq!(Version::new(None).to_string(), NUMBER);
}

#[test]
fn a_text_that_isnt_a_sha_gives_the_version_alone() {
    for text in ["", "3a6dc8", "3a6dc8g", "HEAD", "3a6dc89f\n"] {
        assert_eq!(Version::new(Some(text)).to_string(), NUMBER, "{text:?}");
    }
}
