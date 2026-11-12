use super::*;

/// Each built-in theme's file, name and kind.
fn built_in() -> Vec<(&'static str, &'static str, ThemeKind)> {
    let light = LightTheme::ALL.map(|theme| (theme.file(), theme.name(), ThemeKind::Light));
    let dark = DarkTheme::ALL.map(|theme| (theme.file(), theme.name(), ThemeKind::Dark));
    light.into_iter().chain(dark).collect()
}

#[test]
fn every_built_in_theme_sets_every_token_and_skips_no_line() {
    for (text, name, kind) in built_in() {
        let theme = Theme::parse(text).unwrap();
        assert_eq!(theme.name, name);
        assert_eq!(theme.kind, kind, "{name}");
        assert_eq!(theme.skipped, [], "{name}");
        assert_eq!(
            theme.colors.keys().copied().collect::<Vec<_>>(),
            Token::ALL,
            "{name}"
        );
    }
}

#[test]
fn four_themes_are_built_in_with_vs_codes_the_defaults() {
    let names: Vec<&str> = built_in().into_iter().map(|(_, name, _)| name).collect();
    assert_eq!(
        names,
        [
            "VS Code Light Modern",
            "Gruvbox Light Hard",
            "VS Code Dark Modern",
            "Gruvbox Dark Hard"
        ]
    );
    assert_eq!(LightTheme::default(), LightTheme::VsCodeLightModern);
    assert_eq!(DarkTheme::default(), DarkTheme::VsCodeDarkModern);
    assert_eq!(ThemeMode::default(), ThemeMode::System);
}

#[test]
fn a_side_finds_its_themes_by_name_and_no_other_sides() {
    assert_eq!(
        LightTheme::named("Gruvbox Light Hard"),
        Some(LightTheme::GruvboxLightHard)
    );
    assert_eq!(
        DarkTheme::named("Gruvbox Dark Hard"),
        Some(DarkTheme::GruvboxDarkHard)
    );
    assert_eq!(LightTheme::named("Gruvbox Dark Hard"), None);
    assert_eq!(DarkTheme::named("VS Code Light Modern"), None);
    assert_eq!(LightTheme::named("gruvbox light hard"), None);
    assert_eq!(DarkTheme::named("Solarized Dark"), None);
}

#[test]
fn a_choice_names_a_theme_of_its_own_side() {
    assert_eq!(
        ThemeChoice::named(ThemeKind::Light, "Gruvbox Light Hard").unwrap(),
        ThemeChoice::Light(LightTheme::GruvboxLightHard)
    );
    assert_eq!(
        ThemeChoice::named(ThemeKind::Dark, "VS Code Dark Modern").unwrap(),
        ThemeChoice::Dark(DarkTheme::VsCodeDarkModern)
    );
    for (kind, name, message) in [
        (
            ThemeKind::Light,
            "Gruvbox Dark Hard",
            r#"No light theme is named "Gruvbox Dark Hard"."#,
        ),
        (
            ThemeKind::Dark,
            "Solarized Dark",
            r#"No dark theme is named "Solarized Dark"."#,
        ),
    ] {
        let error = ThemeChoice::named(kind, name).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.message, message);
    }
}

#[test]
fn the_themes_sent_are_each_sides_built_in_ones_with_the_chosen_named_and_the_mode() {
    let themes = Themes::chosen(
        ThemeMode::Dark,
        LightTheme::GruvboxLightHard,
        DarkTheme::GruvboxDarkHard,
    )
    .unwrap();
    assert_eq!(themes.mode, ThemeMode::Dark);
    assert_eq!(themes.light.chosen, "Gruvbox Light Hard");
    assert_eq!(themes.dark.chosen, "Gruvbox Dark Hard");
    // Each with its `bg`, in the order the picker lists them.
    let listed = |side: &SideThemes| -> Vec<(String, String)> {
        side.all
            .iter()
            .map(|view| {
                (
                    view.name.clone(),
                    view.tokens.first().unwrap().value.clone(),
                )
            })
            .collect()
    };
    let pair = |name: &str, bg: &str| (name.to_owned(), bg.to_owned());
    assert_eq!(
        listed(&themes.light),
        [
            pair("VS Code Light Modern", "#ffffff"),
            pair("Gruvbox Light Hard", "#f9f5d7")
        ]
    );
    assert_eq!(
        listed(&themes.dark),
        [
            pair("VS Code Dark Modern", "#1f1f1f"),
            pair("Gruvbox Dark Hard", "#1d2021")
        ]
    );
}

#[test]
fn the_view_carries_every_token_as_a_css_variable() {
    let themes = Themes::chosen(
        ThemeMode::System,
        LightTheme::default(),
        DarkTheme::default(),
    )
    .unwrap();
    for view in themes.light.all.iter().chain(&themes.dark.all) {
        let properties: Vec<String> = view
            .tokens
            .iter()
            .map(|token| token.property.clone())
            .collect();
        let expected: Vec<String> = Token::ALL.into_iter().map(Token::property).collect();
        assert_eq!(properties, expected, "{}", view.name);
    }
    assert!(
        themes
            .light
            .all
            .iter()
            .all(|view| view.kind == ThemeKind::Light)
    );
    assert!(
        themes
            .dark
            .all
            .iter()
            .all(|view| view.kind == ThemeKind::Dark)
    );
    let first = themes.dark.all.first().unwrap().tokens.first().unwrap();
    assert_eq!(
        (first.property.as_str(), first.value.as_str()),
        ("--bg", "#1f1f1f")
    );
}

// The window keeps its own defaults for a token a file leaves out, until #519 builds such a file on the
// built-in Theme of its kind.
#[test]
fn a_file_from_before_the_focus_tokens_still_loads_whole() {
    let text =
        "name = \"Old\"\nkind = \"light\"\n\n[colors]\nbg = \"#ffffff\"\nselected = \"#e8e8e8\"\n";
    let theme = Theme::parse(text).unwrap();
    assert_eq!(theme.skipped, []);
    assert_eq!(
        theme.colors.keys().copied().collect::<Vec<_>>(),
        [Token::Bg, Token::Selected]
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
    let skipped: Vec<(Option<usize>, &str, SkipReason)> = theme
        .skipped
        .iter()
        .map(|line| (line.line, line.key.as_str(), line.reason))
        .collect();
    assert_eq!(
        skipped,
        [
            (Some(3), "font", SkipReason::UnknownKey),
            (Some(7), "bgg", SkipReason::UnknownKey),
            (Some(8), "fg", SkipReason::BadValue),
            (Some(9), "border", SkipReason::BadValue),
            (Some(10), "keyword", SkipReason::UnknownKey),
            (Some(15), "icons", SkipReason::UnknownKey),
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
            line: Some(3),
            key: "colors".to_owned(),
            reason: SkipReason::BadValue
        }]
    );
}

#[test]
fn dotted_keys_name_their_line_too() {
    let theme = Theme::parse("name = \"Mine\"\nkind = \"light\"\ncolors.bgg = \"#fff\"\n").unwrap();
    assert_eq!(
        theme.skipped,
        [SkippedLine {
            line: Some(3),
            key: "bgg".to_owned(),
            reason: SkipReason::UnknownKey
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
    for text in ["kind = \"light\"", "name = \"x\""] {
        let error = Themes::built_in(text).unwrap_err();
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

#[test]
fn a_built_in_theme_must_set_every_token() {
    let partial = "name = \"Part\"\nkind = \"dark\"\n[colors]\nbg = \"#000\"\n";
    let error = Themes::built_in(partial).unwrap_err();
    assert!(
        error
            .message
            .starts_with("A built-in theme can't be read: Part sets no fg, fg_muted, border,"),
        "{}",
        error.message
    );
}

#[test]
fn the_window_takes_the_bg_of_the_built_in_theme_of_its_side() {
    let light = Themes::built_in_background(ThemeKind::Light);
    let dark = Themes::built_in_background(ThemeKind::Dark);
    assert_eq!(light.rgba(), [0xff, 0xff, 0xff, 0xff]);
    assert_eq!(dark.css(), "#1f1f1f");
    assert_eq!(dark.rgba(), [0x1f, 0x1f, 0x1f, 0xff]);
}

/// `docs/devel/themes.md` has a row for each token with every built-in theme's value, so the page can't drift
/// from the files.
#[test]
fn the_doc_page_lists_every_token_with_each_built_in_themes_value() {
    let page = include_str!("../../../../docs/devel/themes.md");
    let themes: Vec<Theme> = built_in()
        .into_iter()
        .map(|(text, _, _)| Theme::parse(text).unwrap())
        .collect();
    for token in Token::ALL {
        let start = format!("| `{}` |", token.key());
        let row = page
            .lines()
            .find(|line| line.starts_with(&start))
            .unwrap_or_else(|| panic!("no row for {}", token.key()));
        let values: Vec<String> = row
            .split('|')
            .skip(3)
            .map(|cell| cell.trim().to_owned())
            .filter(|cell| !cell.is_empty())
            .collect();
        let expected: Vec<String> = themes
            .iter()
            .map(|theme| format!("`{}`", theme.colors[&token].css()))
            .collect();
        assert_eq!(values, expected, "{row}");
    }
}
