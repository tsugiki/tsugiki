use super::*;

#[test]
fn both_built_in_themes_set_every_token_and_skip_no_line() {
    for (text, name, kind) in [
        (LIGHT_MODERN, "VS Code Light Modern", ThemeKind::Light),
        (DARK_MODERN, "VS Code Dark Modern", ThemeKind::Dark),
    ] {
        let theme = Theme::parse(text).unwrap();
        assert_eq!(theme.name, name);
        assert_eq!(theme.kind, kind);
        assert_eq!(theme.skipped, []);
        assert_eq!(
            theme.colors.keys().copied().collect::<Vec<_>>(),
            Token::ALL,
            "{name}"
        );
    }
}

#[test]
fn the_view_carries_every_token_as_a_css_variable() {
    let themes = Themes::built_in().unwrap();
    for view in [&themes.light, &themes.dark] {
        let properties: Vec<String> = view
            .tokens
            .iter()
            .map(|token| token.property.clone())
            .collect();
        let expected: Vec<String> = Token::ALL.into_iter().map(Token::property).collect();
        assert_eq!(properties, expected, "{}", view.name);
    }
    assert_eq!(themes.light.kind, ThemeKind::Light);
    assert_eq!(themes.dark.kind, ThemeKind::Dark);
    let first = themes.dark.tokens.first().unwrap();
    assert_eq!(
        (first.property.as_str(), first.value.as_str()),
        ("--bg", "#1f1f1f")
    );
}

#[test]
fn a_token_becomes_a_css_variable_with_dashes() {
    assert_eq!(Token::Bg.property(), "--bg");
    assert_eq!(Token::FindCurrentFg.property(), "--find-current-fg");
    assert_eq!(Token::Keyword.property(), "--hl-keyword");
}

#[test]
fn colours_come_in_three_lengths_and_css_writes_them_back() {
    let css = |text: &str| text.parse::<Color>().map(Color::css);
    assert_eq!(css("#fA0"), Ok("#ffaa00".to_owned()));
    assert_eq!(css("#1F1f1f"), Ok("#1f1f1f".to_owned()));
    assert_eq!(css("#00000026"), Ok("#00000026".to_owned()));
    assert_eq!(css("#123456ff"), Ok("#123456".to_owned()));
    for bad in [
        "", "#", "fff", "#ff", "#ffff", "#fffffff", "#gggggg", "red", "#ffé",
    ] {
        assert_eq!(css(bad), Err(NotAColor), "{bad:?}");
    }
}

#[test]
fn an_unknown_key_or_a_bad_value_skips_its_line_and_the_rest_loads() {
    let text = "\
name = \"Mine\"
kind = \"dark\"
font = \"Consolas\"

[colors]
bg = \"#000\"
bgg = \"#111\"
fg = \"white\"
border = 3
keyword = \"#222\"

[syntax]
keyword = \"#333\"

[icons]
x = 1
";
    let theme = Theme::parse(text).unwrap();
    let skipped: Vec<(usize, &str, SkipReason)> = theme
        .skipped
        .iter()
        .map(|line| (line.line, line.key.as_str(), line.reason))
        .collect();
    assert_eq!(
        skipped,
        [
            (3, "font", SkipReason::UnknownKey),
            (7, "bgg", SkipReason::UnknownKey),
            (8, "fg", SkipReason::BadValue),
            (9, "border", SkipReason::BadValue),
            (10, "keyword", SkipReason::UnknownKey),
            (15, "icons", SkipReason::UnknownKey),
        ]
    );
    let set: Vec<(Token, String)> = theme
        .colors
        .iter()
        .map(|(token, color)| (*token, color.css()))
        .collect();
    assert_eq!(
        set,
        [
            (Token::Bg, "#000000".to_owned()),
            (Token::Keyword, "#333333".to_owned())
        ]
    );
}

#[test]
fn a_table_that_isnt_a_table_is_skipped() {
    let theme = Theme::parse("name = \"Mine\"\nkind = \"light\"\ncolors = \"#fff\"\n").unwrap();
    assert_eq!(
        theme.skipped,
        [SkippedLine {
            line: 3,
            key: "colors".to_owned(),
            reason: SkipReason::BadValue
        }]
    );
}

#[test]
fn inline_tables_work_too() {
    let theme =
        Theme::parse("name = \"Mine\"\nkind = \"light\"\ncolors = { bg = \"#fff\" }\n").unwrap();
    assert_eq!(theme.skipped, []);
    assert_eq!(theme.colors.len(), 1);
}

#[test]
fn a_file_without_a_name_or_a_kind_fails_whole() {
    assert_eq!(Theme::parse("kind = \"dark\""), Err(ThemeError::NoName));
    assert_eq!(
        Theme::parse("name = 3\nkind = \"dark\""),
        Err(ThemeError::NoName)
    );
    assert_eq!(Theme::parse("name = \"Mine\""), Err(ThemeError::NoKind));
    assert_eq!(
        Theme::parse("name = \"Mine\"\nkind = \"dusk\""),
        Err(ThemeError::NoKind)
    );
    assert_eq!(
        ThemeError::NoKind.to_string(),
        "the theme's `kind` isn't \"light\" or \"dark\""
    );
    assert_eq!(ThemeError::NoName.to_string(), "the theme has no `name`");
}

#[test]
fn a_file_that_isnt_toml_names_the_line() {
    let error = Theme::parse("name = \"Mine\"\nkind = \"dark\"\n[colors\n").unwrap_err();
    let ThemeError::NotToml { line, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(*line, Some(3));
    assert!(error.to_string().starts_with("line 3: "), "{error}");
    let unknown = ThemeError::NotToml {
        line: None,
        message: "broken".to_owned(),
    };
    assert_eq!(unknown.to_string(), "broken");
}

#[test]
fn a_theme_that_cant_be_read_says_why() {
    for (light, dark) in [
        ("kind = \"light\"", DARK_MODERN),
        (LIGHT_MODERN, "name = \"x\""),
    ] {
        let error = Themes::from_files(light, dark).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Internal);
        assert!(
            error
                .message
                .starts_with("A built-in theme can't be read: the theme"),
            "{}",
            error.message
        );
    }
}

/// `docs/devel/themes.md` has a row for each token with both built-in themes' values, so the page can't drift
/// from the files.
#[test]
fn the_doc_page_lists_every_token_with_both_values() {
    let page = include_str!("../../../../docs/devel/themes.md");
    let light = Theme::parse(LIGHT_MODERN).unwrap();
    let dark = Theme::parse(DARK_MODERN).unwrap();
    for token in Token::ALL {
        let start = format!("| `{}` |", token.key());
        let row = page
            .lines()
            .find(|line| line.starts_with(&start))
            .unwrap_or_else(|| panic!("no row for {}", token.key()));
        for theme in [&light, &dark] {
            let value = format!("`{}`", theme.colors[&token].css());
            assert!(row.contains(&value), "{row} lacks {value}");
        }
    }
}
