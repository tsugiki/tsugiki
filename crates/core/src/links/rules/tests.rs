use super::*;
use crate::links::{fixed_under_each, origins_in_title, reads_cherry_picks};

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// The signals of a PR titled `title` with no closing references, and off the default branch with an empty
/// description.
fn off_default(title: &str) -> LinkSignals<'_> {
    LinkSignals {
        title,
        closing_issues: &[],
        off_default_description: Some(""),
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
        let under_cpython: Vec<_> = expected
            .into_iter()
            .map(|number| (number, Some(Conventions::Cpython)))
            .collect();
        let named = fixed_under_each(
            &[LinkRule::CpythonTitleIssues],
            &off_default(title),
            &cpython(),
        );
        assert_eq!(named, under_cpython, "{title}");
    }
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
        let named = origins_in_title(&[LinkRule::CpythonTitleOrigin], &off_default(title));
        let under_cpython: Vec<_> = expected
            .map(|origin| (Conventions::Cpython, origin))
            .into_iter()
            .collect();
        assert_eq!(named, under_cpython, "{title}");
    }
}

#[test]
fn a_title_s_marker_names_no_origin_on_the_default_branch() {
    let signals = LinkSignals {
        off_default_description: None,
        ..off_default("[3.14] gh-1: Fix it (GH-9)")
    };
    assert_eq!(
        origins_in_title(&[LinkRule::CpythonTitleOrigin], &signals),
        []
    );
}

#[test]
fn each_rule_names_only_what_it_reads() {
    let signals = LinkSignals {
        title: "[3.14] gh-1: Fix it (GH-9)",
        closing_issues: &[2],
        off_default_description: Some("Fixes #3"),
    };
    let fixed = LinkRule::SET.map(|rule| fixed_under_each(&[rule], &signals, &cpython()));
    let origins = LinkRule::SET.map(|rule| origins_in_title(&[rule], &signals));
    let picks = LinkRule::SET.map(|rule| reads_cherry_picks(&[rule]));
    assert_eq!(
        fixed,
        [
            vec![(2, None)],
            vec![(3, None)],
            vec![(1, Some(Conventions::Cpython))],
            vec![],
            vec![],
        ]
    );
    assert_eq!(
        origins,
        [
            vec![],
            vec![],
            vec![],
            vec![(Conventions::Cpython, 9)],
            vec![]
        ]
    );
    assert_eq!(picks, [false, false, false, false, true]);
}
