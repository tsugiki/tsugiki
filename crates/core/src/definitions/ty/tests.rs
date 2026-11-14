use super::*;
use crate::definitions::testing::Fixture;

const PATCHLEVEL: &str = "#define PY_MAJOR_VERSION        3\n#define PY_MINOR_VERSION        14\n";

#[test]
fn cpython_s_layout_gives_ty_lib_and_the_version_and_another_gives_it_nothing() {
    let cpython = Fixture::new(
        "definitions-settings",
        &[("Include/patchlevel.h", PATCHLEVEL)],
        &[],
    );
    let other = Fixture::new("definitions-settings-other", &[("a.py", "")], &[]);

    let environment = json!({"root": ["Lib"], "python-version": "3.14"});
    assert_eq!(
        settings(&cpython.checkout.folder),
        json!({"ty": {"configuration": {"environment": environment}}})
    );
    assert_eq!(settings(&other.checkout.folder), json!({}));
}

#[test]
fn a_patchlevel_without_its_version_gives_lib_alone() {
    let fixture = Fixture::new(
        "definitions-no-version",
        &[("Include/patchlevel.h", "#define PY_MAJOR_VERSION 3\n")],
        &[],
    );

    assert_eq!(
        settings(&fixture.checkout.folder),
        json!({"ty": {"configuration": {"environment": {"root": ["Lib"]}}}})
    );
    assert_eq!(python_version("#define PY_MINOR_VERSION 15\n"), None);
    assert_eq!(python_version(PATCHLEVEL).as_deref(), Some("3.14"));
}
