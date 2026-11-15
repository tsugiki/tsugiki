use super::*;
use crate::definitions::testing::Fixture;

const PATCHLEVEL: &str = "#define PY_MAJOR_VERSION        3\n#define PY_MINOR_VERSION        14\n";

/// ty's settings with `environment`.
fn configured(environment: Value) -> Value {
    json!({"ty": {"configuration": {"environment": environment}}})
}

#[test]
fn cpython_s_layout_gives_ty_lib_and_the_version_with_lib_alone_to_fall_back_on() {
    let cpython = Fixture::new(
        "definitions-settings",
        &[("Include/patchlevel.h", PATCHLEVEL)],
        &[],
    );
    let other = Fixture::new("definitions-settings-other", &[("a.py", "")], &[]);

    let (given, fallback) = settings(&cpython.checkout.folder);
    let fallback = fallback.unwrap();
    let (elsewhere, none) = settings(&other.checkout.folder);

    let environment = json!({"root": ["Lib"], "python-version": "3.14"});
    assert_eq!(given, configured(environment));
    assert_eq!(fallback.settings, configured(json!({"root": ["Lib"]})));
    // ty 0.0.103's error over this version holds the refusal, and its error over another version doesn't.
    let shown = |version: &str| {
        format!(
            "Invalid `ty.configuration` options: unknown variant `{version}`, expected one of `3.7`"
        )
    };
    let refused = |message: String| (fallback.refusal.iter()).all(|text| message.contains(text));
    assert!(refused(shown("3.14")));
    assert!(!refused(shown("3.16")));
    assert_eq!(elsewhere, json!({}));
    assert!(none.is_none());
}

#[test]
fn a_patchlevel_without_its_version_gives_lib_alone_and_nothing_to_fall_back_on() {
    let fixture = Fixture::new(
        "definitions-no-version",
        &[("Include/patchlevel.h", "#define PY_MAJOR_VERSION 3\n")],
        &[],
    );

    let (given, fallback) = settings(&fixture.checkout.folder);

    assert_eq!(given, configured(json!({"root": ["Lib"]})));
    assert!(fallback.is_none());
    assert_eq!(python_version("#define PY_MINOR_VERSION 15\n"), None);
    assert_eq!(python_version(PATCHLEVEL).as_deref(), Some("3.14"));
}
