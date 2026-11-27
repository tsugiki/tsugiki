use super::*;

fn from_toml(text: &str) -> Result<Filter, toml_edit::de::Error> {
    toml_edit::de::from_str(text)
}

#[test]
fn a_person_s_search_names_them_as_the_author_and_open() {
    assert_eq!(
        FilterQuery::person("octocat").as_str(),
        "is:open author:octocat"
    );
}

#[test]
fn a_repository_starts_with_mine_on_the_workbench_and_involved_off_it() {
    let queries: Vec<(String, String, OnWorkbench)> = Filter::starters()
        .into_iter()
        .map(|filter| (filter.name.into(), filter.query.into(), filter.on_workbench))
        .collect();
    assert_eq!(
        queries,
        [
            (
                "Mine".to_owned(),
                "is:open author:@me".to_owned(),
                OnWorkbench::On
            ),
            (
                "Involved".to_owned(),
                "is:open involves:@me -author:@me".to_owned(),
                OnWorkbench::Off
            ),
        ]
    );
}

#[test]
fn a_filter_from_the_file_loses_the_spaces_around_it() {
    let filter = from_toml("name = ' Bugs '\nquery = ' label:bug '").unwrap();
    assert_eq!(
        filter,
        Filter::starter("Bugs", "label:bug", OnWorkbench::Off)
    );
    assert_eq!(filter.query.as_str(), "label:bug");
}

#[test]
fn a_filter_reads_its_switch_and_is_off_the_workbench_without_one() {
    let on = from_toml("name = 'Bugs'\nquery = 'label:bug'\non_workbench = true").unwrap();
    assert_eq!(on, Filter::starter("Bugs", "label:bug", OnWorkbench::On));
    let off = from_toml("name = 'Bugs'\nquery = 'label:bug'\non_workbench = false").unwrap();
    assert_eq!(off.on_workbench, OnWorkbench::Off);
    let unsaid = from_toml("name = 'Bugs'\nquery = 'label:bug'").unwrap();
    assert_eq!(unsaid.on_workbench, OnWorkbench::Off);
}

#[test]
fn a_filter_writes_its_switch_as_a_bool() {
    let written = toml_edit::ser::to_string(&Filter::starter("Bugs", "label:bug", OnWorkbench::On));
    assert_eq!(
        written.unwrap(),
        "name = \"Bugs\"\nquery = \"label:bug\"\non_workbench = true\n"
    );
}

#[test]
fn a_token_names_a_state_or_a_sort_as_tsugiki_reads_it() {
    for token in [
        "is:open",
        "IS:Closed",
        "is:merged",
        "is:unmerged",
        "state:closed",
        "-is:open",
        "-state:merged",
    ] {
        assert!(names_a_state(token), "{token}");
    }
    for token in ["is:issue", "is:pr", "is:draft", "-is:issue", "-fix", "fix"] {
        assert!(!names_a_state(token), "{token}");
    }
    assert!(names_a_sort("sort:updated-desc"));
    assert!(names_a_sort("SORT:created-asc"));
    assert!(!names_a_sort("is:open"));
    assert!(!names_a_sort("-sort:updated"));
}

#[test]
fn a_filter_query_spells_out_is_open_unless_the_text_names_a_state() {
    let spelled =
        |text: &str| FilterQuery::with_additions(text).map(|query| query.as_str().to_owned());
    assert_eq!(spelled("label:bug"), Some("label:bug is:open".to_owned()));
    assert_eq!(
        spelled("  author:@me  "),
        Some("author:@me is:open".to_owned())
    );
    assert_eq!(
        spelled("is:closed label:bug"),
        Some("is:closed label:bug".to_owned())
    );
    assert_eq!(
        spelled("-is:open label:bug"),
        Some("-is:open label:bug".to_owned())
    );
    assert_eq!(
        spelled("state:merged label:bug"),
        Some("state:merged label:bug".to_owned())
    );
    assert_eq!(spelled("   "), None);
}

#[test]
fn a_blank_name_or_query_is_refused_and_says_which() {
    let no_name = from_toml("name = '  '\nquery = 'label:bug'").unwrap_err();
    assert!(
        no_name
            .to_string()
            .contains("A Filter's name can't be blank.")
    );
    let no_query = from_toml("name = 'Bugs'\nquery = ''").unwrap_err();
    assert!(
        no_query
            .to_string()
            .contains("A Filter's query can't be blank.")
    );
}
