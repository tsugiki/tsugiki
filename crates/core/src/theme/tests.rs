use std::path::PathBuf;

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

/// A `themes` folder in `.temp` holding `files`, each a file name and its text.
fn folder_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let folder = crate::testing::temp_path(name);
    std::fs::create_dir_all(&folder).unwrap();
    for (file, text) in files {
        std::fs::write(folder.join(file), text).unwrap();
    }
    folder
}

/// The library of a folder holding `files`, which it then removes.
fn library_of(name: &str, files: &[(&str, &str)]) -> (ThemeLibrary, PathBuf) {
    let folder = folder_with(name, files);
    let library = ThemeLibrary::read(&folder).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    (library, folder)
}

/// The themes the library sends with the defaults chosen.
fn sent(library: &ThemeLibrary) -> Themes {
    library.themes(
        ThemeMode::System,
        &ThemeName::default_of(ThemeKind::Light),
        &ThemeName::default_of(ThemeKind::Dark),
    )
}

/// The value of `property` in the theme of side `kind` named `name`.
fn value(themes: &Themes, kind: ThemeKind, name: &str, property: &str) -> String {
    let side = match kind {
        ThemeKind::Light => &themes.light,
        ThemeKind::Dark => &themes.dark,
    };
    let theme = side.all.iter().find(|theme| theme.name == name).unwrap();
    let token = theme.tokens.iter().find(|token| token.property == property);
    token.unwrap().value.clone()
}

fn names(side: &SideThemes) -> Vec<&str> {
    side.all.iter().map(|theme| theme.name.as_str()).collect()
}

/// Each failed file of a side as its name, its file and why.
fn failed(side: &SideThemes) -> Vec<(&str, &str, &str)> {
    side.failed
        .iter()
        .map(|theme| {
            (
                theme.name.as_str(),
                theme.file.as_str(),
                theme.reason.as_str(),
            )
        })
        .collect()
}

#[test]
fn a_file_of_a_few_colours_takes_the_rest_from_its_kinds_default_theme() {
    let (library, _) = library_of(
        "themes-partial",
        &[(
            "solarized.toml",
            "name = \"Solarized Dark\"\nkind = \"dark\"\n\n[colors]\nbg = \"#002b36\"\n\n\
             [syntax]\nkeyword = \"#859900\"\n",
        )],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.dark),
        ["VS Code Dark Modern", "Gruvbox Dark Hard", "Solarized Dark"]
    );
    assert_eq!(
        names(&themes.light),
        ["VS Code Light Modern", "Gruvbox Light Hard"]
    );
    let solarized = |property| value(&themes, ThemeKind::Dark, "Solarized Dark", property);
    assert_eq!(solarized("--bg"), "#002b36");
    assert_eq!(solarized("--hl-keyword"), "#859900");
    // The rest from VS Code Dark Modern, so every token has a value.
    assert_eq!(solarized("--fg"), "#cccccc");
    assert_eq!(solarized("--hl-string"), "#ce9178");
    let solarized = themes.dark.all.last().unwrap();
    assert_eq!(solarized.tokens.len(), Token::ALL.len());
    assert_eq!(themes.problems, Vec::<String>::new());
}

/// A `themes` that is a file, or a folder Tsugiki may not list, would otherwise drop the user's themes
/// without a word.
#[test]
fn a_themes_folder_that_cant_be_listed_is_named_in_the_problems() {
    let file = crate::testing::temp_path("themes-a-file");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "not a folder").unwrap();
    let themes = sent(&ThemeLibrary::read(&file).unwrap());
    std::fs::remove_file(&file).unwrap();

    assert_eq!(
        names(&themes.dark),
        ["VS Code Dark Modern", "Gruvbox Dark Hard"]
    );
    let unread = format!("Tsugiki can't read {}: ", file.display());
    assert_eq!(themes.problems.len(), 1, "{:?}", themes.problems);
    assert!(
        themes.problems[0].starts_with(&unread),
        "{:?}",
        themes.problems
    );
}

#[test]
fn a_theme_that_extends_one_of_the_other_side_fails() {
    let (library, _) = library_of(
        "themes-other-side",
        &[
            ("dusk.toml", "name = \"Dusk\"\nkind = \"dark\"\n"),
            (
                "noon.toml",
                "name = \"Noon\"\nkind = \"light\"\nextends = \"VS Code Dark Modern\"\n",
            ),
            (
                "dawn.toml",
                "name = \"Dawn\"\nkind = \"light\"\nextends = \"Dusk\"\n",
            ),
        ],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.light),
        ["VS Code Light Modern", "Gruvbox Light Hard"]
    );
    assert_eq!(
        failed(&themes.light),
        [
            ("Dawn", "dawn.toml", "it extends Dusk, a dark theme"),
            (
                "Noon",
                "noon.toml",
                "it extends VS Code Dark Modern, a dark theme"
            ),
        ]
    );
}

#[test]
fn extends_takes_the_colours_of_a_built_in_theme_or_of_another_file() {
    let (library, _) = library_of(
        "themes-extends",
        &[
            (
                "a.toml",
                "name = \"Warm\"\nkind = \"dark\"\nextends = \"Gruvbox Dark Hard\"\n[colors]\nbg = \"#101010\"\n",
            ),
            (
                "b.toml",
                "name = \"Warmer\"\nkind = \"dark\"\nextends = \"Warm\"\n[colors]\nfg = \"#fafafa\"\n",
            ),
        ],
    );
    let themes = sent(&library);
    let warmer = |property| value(&themes, ThemeKind::Dark, "Warmer", property);

    assert_eq!(warmer("--fg"), "#fafafa");
    assert_eq!(warmer("--bg"), "#101010");
    assert_eq!(warmer("--accent"), "#83a598");
    assert_eq!(value(&themes, ThemeKind::Dark, "Warm", "--fg"), "#ebdbb2");
}

#[test]
fn an_extends_loop_or_a_name_no_theme_has_fails_the_file_and_the_picker_says_why() {
    let theme = |name: &str, extends: &str| {
        format!("name = \"{name}\"\nkind = \"light\"\nextends = \"{extends}\"\n")
    };
    let (library, _) = library_of(
        "themes-loop",
        &[
            ("1.toml", &theme("Ping", "Pong")),
            ("2.toml", &theme("Pong", "Ping")),
            ("3.toml", &theme("Lost", "Solarized Light")),
            ("4.toml", &theme("After", "Ping")),
            ("5.toml", &theme("Fine", "Gruvbox Light Hard")),
        ],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.light),
        ["VS Code Light Modern", "Gruvbox Light Hard", "Fine"]
    );
    assert_eq!(
        failed(&themes.light),
        [
            ("After", "4.toml", "it extends Ping, which can't be used"),
            (
                "Lost",
                "3.toml",
                "it extends Solarized Light, which no theme is named"
            ),
            ("Ping", "1.toml", "it extends itself: Ping, Pong, Ping"),
            ("Pong", "2.toml", "it extends itself: Pong, Ping, Pong"),
        ]
    );
    assert_eq!(themes.dark.failed, []);
}

#[test]
fn a_name_another_theme_has_fails_the_later_file() {
    let theme = |name: &str| format!("name = \"{name}\"\nkind = \"dark\"\n");
    let (library, _) = library_of(
        "themes-same-name",
        &[
            ("a.toml", &theme("Night")),
            ("b.toml", &theme("Night")),
            ("c.toml", &theme("Gruvbox Dark Hard")),
        ],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.dark),
        ["VS Code Dark Modern", "Gruvbox Dark Hard", "Night"]
    );
    assert_eq!(
        failed(&themes.dark),
        [
            (
                "Gruvbox Dark Hard",
                "c.toml",
                "a built-in theme has this name"
            ),
            ("Night", "b.toml", "a.toml names a theme Night first"),
        ]
    );
}

#[test]
fn files_come_after_the_built_in_themes_in_name_order_and_only_toml_files_count() {
    let theme = |name: &str| format!("name = \"{name}\"\nkind = \"light\"\n");
    let (library, _) = library_of(
        "themes-order",
        &[
            ("z.toml", &theme("Apricot")),
            ("a.toml", &theme("melon")),
            ("b.TOML", &theme("Lime")),
            ("notes.txt", &theme("Not a theme")),
        ],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.light),
        [
            "VS Code Light Modern",
            "Gruvbox Light Hard",
            "Apricot",
            "Lime",
            "melon"
        ]
    );
}

#[test]
fn with_no_themes_folder_the_built_in_themes_are_all() {
    let folder = crate::testing::temp_path("themes-none");
    let themes = sent(&ThemeLibrary::read(&folder).unwrap());

    assert_eq!(
        names(&themes.light),
        ["VS Code Light Modern", "Gruvbox Light Hard"]
    );
    assert_eq!(
        names(&themes.dark),
        ["VS Code Dark Modern", "Gruvbox Dark Hard"]
    );
    assert_eq!(themes.problems, Vec::<String>::new());
}

#[test]
fn a_bad_colour_or_an_unknown_key_skips_its_line_and_names_the_file_the_line_and_the_key() {
    let (library, folder) = library_of(
        "themes-skipped",
        &[(
            "mine.toml",
            "name = \"Mine\"\nkind = \"light\"\n\n[colors]\nbg = \"#fff\"\nfg = \"black\"\nglow = \"#000\"\n\
             accent = \"#11223380\"\nborder = \"#aabbcc\"\n",
        )],
    );
    let themes = sent(&library);
    let mine = |property| value(&themes, ThemeKind::Light, "Mine", property);

    assert_eq!(mine("--bg"), "#ffffff");
    assert_eq!(mine("--accent"), "#11223380");
    assert_eq!(mine("--border"), "#aabbcc");
    assert_eq!(mine("--fg"), "#3b3b3b");
    let path = folder.join("mine.toml");
    assert_eq!(
        themes.problems,
        [
            format!(
                "{}, line 6: fg isn't a colour, so Tsugiki skips it.",
                path.display()
            ),
            format!(
                "{}, line 7: glow names no token, so Tsugiki skips it.",
                path.display()
            ),
        ]
    );
}

#[test]
fn a_file_that_isnt_a_theme_stays_out_of_the_lists_and_is_named() {
    let (library, folder) = library_of(
        "themes-broken",
        &[
            ("broken.toml", "name = \"Broken\"\nkind = \n"),
            ("kindless.toml", "name = \"Kindless\"\n"),
        ],
    );
    let themes = sent(&library);

    assert_eq!(
        names(&themes.light),
        ["VS Code Light Modern", "Gruvbox Light Hard"]
    );
    assert_eq!(themes.light.failed, []);
    assert_eq!(themes.problems.len(), 2);
    let broken = folder.join("broken.toml").display().to_string();
    assert!(
        themes.problems[0].starts_with(&format!("{broken}, line 2: ")),
        "{:?}",
        themes.problems
    );
    assert_eq!(
        themes.problems[1],
        format!(
            "{}: the theme's `kind` isn't \"light\" or \"dark\"",
            folder.join("kindless.toml").display()
        )
    );
}

#[test]
fn a_file_tsugiki_cant_read_is_named() {
    let folder = folder_with("themes-unreadable", &[]);
    std::fs::create_dir_all(folder.join("folder.toml")).unwrap();
    let themes = sent(&ThemeLibrary::read(&folder).unwrap());
    std::fs::remove_dir_all(&folder).unwrap();

    let unread = format!(
        "Tsugiki can't read {}: ",
        folder.join("folder.toml").display()
    );
    assert_eq!(themes.problems.len(), 1, "{:?}", themes.problems);
    assert!(
        themes.problems[0].starts_with(&unread),
        "{:?}",
        themes.problems
    );
}

#[test]
fn extends_names_a_theme_with_text_or_its_line_is_skipped() {
    let theme = Theme::parse("name = \"Mine\"\nkind = \"dark\"\nextends = 3\n").unwrap();
    assert_eq!(theme.extends, None);
    assert_eq!(
        theme.skipped,
        [SkippedLine {
            line: Some(3),
            key: "extends".to_owned(),
            reason: SkipReason::NotAName,
        }]
    );
    let line = skipped(Path::new("mine.toml"), &theme.skipped[0]);
    assert_eq!(
        line,
        "mine.toml, line 3: extends isn't a theme's name, so Tsugiki skips it."
    );
    let theme = Theme::parse("name = \"Mine\"\nkind = \"dark\"\nextends = \"Gruvbox Dark Hard\"\n");
    assert_eq!(
        theme.unwrap().extends,
        Some(ThemeName::new("Gruvbox Dark Hard"))
    );
}

#[test]
fn the_library_finds_a_theme_by_its_side_and_its_name() {
    let (library, _) = library_of(
        "themes-named",
        &[("mine.toml", "name = \"Mine\"\nkind = \"dark\"\n")],
    );

    assert_eq!(
        library.named(ThemeKind::Dark, "Mine").unwrap(),
        ThemeName::new("Mine")
    );
    assert_eq!(
        library
            .named(ThemeKind::Light, "Gruvbox Light Hard")
            .unwrap(),
        ThemeName::new("Gruvbox Light Hard")
    );
    for (kind, name, message) in [
        (
            ThemeKind::Light,
            "Mine",
            r#"No light theme is named "Mine"."#,
        ),
        (
            ThemeKind::Dark,
            "gruvbox dark hard",
            r#"No dark theme is named "gruvbox dark hard"."#,
        ),
    ] {
        let error = library.named(kind, name).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.message, message);
    }
}

#[test]
fn a_chosen_theme_whose_file_has_gone_gives_way_to_the_default_and_says_so() {
    let (library, _) = library_of("themes-gone", &[]);
    let themes = library.themes(
        ThemeMode::Dark,
        &ThemeName::new("Gruvbox Light Hard"),
        &ThemeName::new("Solarized Dark"),
    );

    assert_eq!(themes.light.chosen, "Gruvbox Light Hard");
    assert_eq!(themes.dark.chosen, "VS Code Dark Modern");
    assert_eq!(
        themes.problems,
        ["No dark theme is named \"Solarized Dark\", so Tsugiki shows VS Code Dark Modern."]
    );
}

#[test]
fn the_themes_sent_are_each_sides_built_in_ones_with_the_chosen_named_and_the_mode() {
    let (library, _) = library_of("themes-built-in", &[]);
    let themes = library.themes(
        ThemeMode::Dark,
        &ThemeName::new("Gruvbox Light Hard"),
        &ThemeName::new("Gruvbox Dark Hard"),
    );
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
    let (library, _) = library_of("themes-view", &[]);
    let themes = sent(&library);
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

// A file's own colours are the tokens it sets; `ThemeLibrary` fills in the rest from the theme it builds on.
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
