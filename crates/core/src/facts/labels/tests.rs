use super::*;

#[test]
fn label_names_ignore_case_as_github_does() {
    assert_eq!(Label::new("Type-Bug"), Label::new("type-bug"));
    assert_ne!(Label::new("type-bug"), Label::new("type-feature"));
}
