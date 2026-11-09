use super::*;

fn from_toml(text: &str) -> Result<Filter, toml_edit::de::Error> {
    toml_edit::de::from_str(text)
}

#[test]
fn a_maintained_repository_starts_with_everything_open() {
    let queries: Vec<(String, String)> = Filter::starters(RepositoryAccess::Maintained)
        .into_iter()
        .map(|filter| (filter.name.into(), filter.query.into()))
        .collect();
    assert_eq!(
        queries,
        [
            ("Open Issues".to_owned(), "is:issue is:open".to_owned()),
            ("Open PRs".to_owned(), "is:pr is:open".to_owned()),
        ]
    );
}

#[test]
fn any_other_repository_starts_with_the_users_own_work() {
    let queries: Vec<(String, String)> = Filter::starters(RepositoryAccess::Other)
        .into_iter()
        .map(|filter| (filter.name.into(), filter.query.into()))
        .collect();
    assert_eq!(
        queries,
        [
            ("Mine".to_owned(), "is:open author:@me".to_owned()),
            (
                "Involved".to_owned(),
                "is:open involves:@me -author:@me".to_owned()
            ),
        ]
    );
}

#[test]
fn a_filter_from_the_file_loses_the_spaces_around_it() {
    let filter = from_toml("name = ' Bugs '\nquery = ' label:bug '").unwrap();
    assert_eq!(filter, Filter::starter("Bugs", "label:bug"));
    assert_eq!(filter.query.as_str(), "label:bug");
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
