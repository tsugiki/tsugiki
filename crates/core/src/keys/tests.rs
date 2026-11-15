use super::*;

fn binding(action: Action, key: Option<Keys>, context: Context) -> Binding {
    Binding {
        action,
        title: "",
        key,
        context,
    }
}

/// The key `text` names, which a test means to be one.
fn keys(text: &str) -> Option<Keys> {
    Some(Keys::parse(text).unwrap_or_else(|| panic!("{text:?} isn't a key")))
}

/// Stricter than a clash within a context: the window finds a key's action without asking where the focus
/// is, so no default key may stand on two actions even in contexts apart, nor start another's chord. A
/// remap that does will need the window to look a key up by context.
#[test]
fn no_two_default_keys_clash_in_any_context() {
    for (index, mine) in TABLE.iter().enumerate() {
        for theirs in TABLE.iter().skip(index + 1) {
            if let (Some(my_key), Some(their_key)) = (mine.key, theirs.key) {
                assert!(
                    !my_key.clashes(their_key),
                    "{:?} and {:?} share {my_key}",
                    mine.action,
                    theirs.action
                );
            }
        }
    }
}

#[test]
fn the_same_key_clashes_where_the_contexts_meet() {
    let e = keys("e");
    let archive = binding(Action::Archive, e, Context::IssueList);
    let find = binding(Action::FindInDiff, e, Context::Diff);

    assert!(archive.clashes(&binding(Action::Mute, e, Context::IssueList)));
    assert!(archive.clashes(&binding(Action::Mute, e, Context::Anywhere)));
    assert!(binding(Action::Mute, e, Context::Anywhere).clashes(&archive));
    assert!(!archive.clashes(&find));
    assert!(!archive.clashes(&binding(Action::SendComment, e, Context::Composer)));
}

#[test]
fn the_composer_shares_the_diff_it_sits_in() {
    let ctrl_enter = keys("ctrl+enter");
    let send = binding(Action::SendComment, ctrl_enter, Context::Composer);
    let find = binding(Action::FindInDiff, ctrl_enter, Context::Diff);

    assert!(send.clashes(&find));
    assert!(find.clashes(&send));
}

#[test]
fn different_keys_never_clash() {
    let anywhere = |action, key| binding(action, keys(key), Context::Anywhere);

    assert!(!anywhere(Action::Archive, "e").clashes(&anywhere(Action::Mute, "m")));
    assert!(!anywhere(Action::LocalMark, "ctrl+k m").clashes(&anywhere(Action::Pin, "ctrl+k p")));
    assert!(anywhere(Action::LocalMark, "ctrl+k m").clashes(&anywhere(Action::Pin, "ctrl+k m")));
}

#[test]
fn a_key_clashes_with_the_chords_it_starts() {
    let ctrl_k = binding(Action::Archive, keys("ctrl+k"), Context::Anywhere);
    let local_mark = binding(Action::LocalMark, keys("ctrl+k m"), Context::Anywhere);
    let ctrl_m = binding(Action::Mute, keys("ctrl+m"), Context::Anywhere);

    assert!(ctrl_k.clashes(&local_mark));
    assert!(local_mark.clashes(&ctrl_k));
    assert!(!ctrl_m.clashes(&local_mark));
}

#[test]
fn an_action_with_no_key_clashes_with_none() {
    let reset = binding(Action::ResetLayout, None, Context::Anywhere);
    let e = binding(Action::Archive, keys("e"), Context::Anywhere);

    assert!(!reset.clashes(&e));
    assert!(!e.clashes(&reset));
}

/// The key `Keys::parse` reads in `text`, named as the window names it.
fn read(text: &str) -> Option<String> {
    Keys::parse(text).map(|key| key.to_string())
}

#[test]
fn a_key_reads_in_any_case_with_its_modifiers_in_any_order() {
    let read_as = [
        ("e", "e"),
        ("E", "e"),
        ("Shift+Q", "shift+q"),
        ("ctrl+e", "ctrl+e"),
        ("Alt+PageDown", "alt+pagedown"),
        ("shift+ctrl+p", "ctrl+shift+p"),
        ("alt+ctrl+u", "ctrl+alt+u"),
        ("shift+alt+x", "alt+shift+x"),
        ("shift+alt+ctrl+x", "ctrl+alt+shift+x"),
        ("cmd+option+b", "ctrl+alt+b"),
        ("Ctrl+K M", "ctrl+k m"),
        ("  ctrl+k   m ", "ctrl+k m"),
        ("ctrl+,", "ctrl+,"),
        ("ctrl++", "ctrl++"),
        ("+", "+"),
        ("?", "?"),
        ("ß", "ß"),
        ("alt+←", "alt+arrowleft"),
        ("alt+→", "alt+arrowright"),
        ("ctrl+↑", "ctrl+arrowup"),
        ("ctrl+↓", "ctrl+arrowdown"),
        ("ctrl+esc", "ctrl+escape"),
    ];
    for (text, name) in read_as {
        assert_eq!(read(text).as_deref(), Some(name), "{text:?}");
    }
}

#[test]
fn every_named_key_reads_by_the_name_the_window_gives_it() {
    for named in Named::ALL {
        assert_eq!(read(named.name()).as_deref(), Some(named.name()));
    }
}

#[test]
fn text_that_names_no_key_reads_as_none() {
    for text in [
        "",
        " ",
        "ctrl+",
        "+e",
        "++",
        "ctrl+ctrl+e",
        "cmd+ctrl+e",
        "hyper+e",
        "ctrl+k m n",
        "ctrl+k ctrl+",
        "ee",
        "pagedwn",
        "\u{7}",
    ] {
        assert_eq!(read(text), None, "{text:?}");
    }
}

#[test]
fn each_actions_binding_is_its_row_of_the_table() {
    for row in TABLE {
        assert_eq!(row.action.binding(), *row);
    }
}

#[test]
fn each_action_reads_by_its_name_in_keys() {
    for row in TABLE {
        let name = serde_json::to_value(row.action).unwrap();
        assert_eq!(Action::parse(name.as_str().unwrap()), Some(row.action));
    }
    assert_eq!(Action::parse("Archive"), None);
}

/// The remaps `Remaps::read` reads in `lines`, in the JSON the window reads and in their order, and the
/// problems.
fn remapped(lines: &[(&str, Option<&str>)]) -> (String, Vec<(String, String)>) {
    let lines = lines
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.map(str::to_owned)))
        .collect();
    let remaps = Remaps::read(lines);
    (
        serde_json::to_string(&remaps).unwrap(),
        remaps.unread().to_vec(),
    )
}

#[test]
fn a_remap_gives_its_action_another_key_or_none_in_the_files_order() {
    let (remaps, unread) = remapped(&[
        ("local_mark", Some("")),
        ("archive", Some("A")),
        ("pin", Some("Ctrl+J P")),
        ("next_tab", Some("ctrl+tab")),
    ]);

    assert_eq!(unread, Vec::new());
    assert_eq!(
        remaps,
        r#"{"local_mark":"","archive":"a","pin":"ctrl+j p","next_tab":"ctrl+tab"}"#
    );
}

#[test]
fn a_line_tsugiki_cant_read_leaves_its_action_at_its_default_and_says_why() {
    let (remaps, unread) = remapped(&[
        ("frobnicate", Some("f")),
        ("archive", None),
        ("mute", Some("ctrl+frob")),
        ("local_mark", Some("tab")),
        ("pin", Some("shift+space ctrl+p")),
        ("reset_layout", Some("pagedown")),
        ("open_devtools", Some("f10")),
    ]);

    assert_eq!(remaps, "{}");
    let problems: Vec<(&str, &str)> = unread
        .iter()
        .map(|(name, problem)| (name.as_str(), problem.as_str()))
        .collect();
    assert_eq!(
        problems,
        [
            (
                "frobnicate",
                "isn't an action Tsugiki has, so Tsugiki skips the line."
            ),
            (
                "archive",
                r#"needs a key in quotes, such as "ctrl+k m", so Archive keeps "e"."#
            ),
            (
                "mute",
                r#"gives "ctrl+frob", which Tsugiki can't read as a key, so Mute keeps "m"."#
            ),
            (
                "local_mark",
                r#"gives "tab", but without Ctrl or Alt that key keeps its own work, so Local Mark keeps "ctrl+k m"."#
            ),
            (
                "pin",
                r#"gives "shift+space ctrl+p", but without Ctrl or Alt that key keeps its own work, so Pin or Unpin keeps "ctrl+k p"."#
            ),
            (
                "reset_layout",
                "gives \"pagedown\", but without Ctrl or Alt that key keeps its own work, so Reset Layout keeps \
                 no key."
            ),
            (
                "open_devtools",
                "can't be remapped, as WebView2 opens DevTools on its own Ctrl+Shift+I in a development build, \
                 so Open DevTools keeps \"ctrl+shift+i\"."
            ),
        ]
    );
}

/// The window names a key that types by what it types, "?" and never "shift+/", so a line with Shift and a
/// symbol or digit would never match a press.
#[test]
fn shift_with_a_symbol_or_digit_is_refused_with_the_character_to_write() {
    let (remaps, unread) = remapped(&[
        ("show_keys", Some("shift+/")),
        ("zoom_in", Some("ctrl+shift+=")),
        ("mark_seen", Some("ctrl+k shift+1")),
        ("mute", Some("shift+ö")),
        ("archive", Some("shift+a")),
    ]);

    assert_eq!(remaps, r#"{"archive":"shift+a"}"#);
    let problems: Vec<&str> = unread.iter().map(|(_, problem)| problem.as_str()).collect();
    let refused = "but the window names a key that types by what it types, with no Shift: write";
    assert_eq!(
        problems,
        [
            format!(
                r#"gives "shift+/", {refused} "?", which Shift+/ types on a US keyboard, so List the View's Keys keeps "?"."#
            ),
            format!(
                r#"gives "ctrl+shift+=", {refused} "+", which Shift+= types on a US keyboard, so Zoom In keeps "ctrl+="."#
            ),
            format!(
                r#"gives "ctrl+k shift+1", {refused} "!", which Shift+1 types on a US keyboard, so Mark Seen keeps "q"."#
            ),
            format!(r#"gives "shift+ö", {refused} what Shift+ö types, so Mute keeps "m"."#),
        ]
    );
}

/// WebView2 opens DevTools on Ctrl+Shift+I in a development build before Tsugiki sees the press, so no key
/// can start with it, as one press or a chord's first. A chord's second press goes to the chord's wait.
#[test]
fn no_key_starts_with_ctrl_shift_i() {
    let (remaps, unread) = remapped(&[
        ("pin", Some("Ctrl+Shift+I")),
        ("archive", Some("ctrl+shift+i x")),
        ("mute", Some("ctrl+k ctrl+shift+i")),
    ]);

    assert_eq!(remaps, r#"{"mute":"ctrl+k ctrl+shift+i"}"#);
    let problems: Vec<&str> = unread.iter().map(|(_, problem)| problem.as_str()).collect();
    let refused = "but WebView2 opens DevTools on Ctrl+Shift+I in a development build";
    assert_eq!(
        problems,
        [
            format!(r#"gives "Ctrl+Shift+I", {refused}, so Pin or Unpin keeps "ctrl+k p"."#),
            format!(r#"gives "ctrl+shift+i x", {refused}, so Archive keeps "e"."#),
        ]
    );
}

/// The chord's wait keeps its second press from the browser and every other handler, so that press may be
/// any key.
#[test]
fn a_chords_second_press_may_be_a_key_that_moves_around() {
    let (remaps, unread) = remapped(&[
        ("pin", Some("ctrl+k arrowdown")),
        ("archive", Some("space")),
    ]);

    assert_eq!(remaps, r#"{"pin":"ctrl+k arrowdown"}"#);
    assert_eq!(unread.len(), 1, "{unread:?}");
}

/// The keys that move around can't be remapped, but a line may still give an action its own default.
#[test]
fn a_line_may_give_an_action_its_default_key_that_moves_around() {
    let (remaps, unread) =
        remapped(&[("unfold", Some("ArrowRight")), ("fold", Some("arrowright"))]);

    assert_eq!(remaps, r#"{"unfold":"arrowright"}"#);
    assert_eq!(unread.len(), 1, "{unread:?}");
}

#[test]
fn the_window_reads_each_action_by_its_name_in_keys() {
    let rows = serde_json::to_value(TABLE).expect("serialize the table");
    let row = |action: &str| {
        rows.as_array()
            .and_then(|rows| rows.iter().find(|row| row["action"] == action))
            .cloned()
            .unwrap_or_else(|| panic!("no row for {action}"))
    };

    assert_eq!(
        row("local_mark"),
        serde_json::json!({"action": "local_mark", "title": "Local Mark", "key": "ctrl+k m", "context": "anywhere"})
    );
    assert_eq!(row("reset_layout")["key"], serde_json::Value::Null);
    assert_eq!(row("next_file")["context"], "diff");
    assert_eq!(row("unfold")["context"], "issue_list");
    assert_eq!(row("send_comment")["context"], "composer");
}

/// Each row of `docs/devel/keyboard.md`'s table, by its Action cell, and the actions whose keys its Key
/// column lists, in its order.
const DOCUMENTED: &[(&str, &[Action])] = &[
    ("Command palette", &[Action::CommandPalette]),
    ("Quick open", &[Action::QuickOpen]),
    (
        "Go to definition / Definition peek",
        &[Action::GoToDefinition, Action::PeekDefinition],
    ),
    (
        "Next / previous change",
        &[Action::NextChange, Action::PreviousChange],
    ),
    (
        "Next / previous comment thread",
        &[Action::NextThread, Action::PreviousThread],
    ),
    ("Find in diff", &[Action::FindInDiff]),
    ("Hide / show the Sidebar", &[Action::ToggleSidebar]),
    ("Hide / show the Issue list", &[Action::ToggleIssueList]),
    (
        "Unfold / fold an Issue's PRs in the Issue list",
        &[Action::Unfold, Action::Fold],
    ),
    (
        "Show only the Detail pane / show the panes",
        &[Action::ToggleDetailOnly],
    ),
    ("Unified ↔ side-by-side", &[Action::ToggleDiffView]),
    ("All files ↔ one file at a time", &[Action::ToggleFilesMode]),
    (
        "Open the files unchanged since a Stale review",
        &[Action::UnfoldUnchanged],
    ),
    (
        "Next / previous tab in the PR view",
        &[Action::NextTab, Action::PreviousTab],
    ),
    (
        "Next / previous file in the Files tab",
        &[Action::NextFile, Action::PreviousFile],
    ),
    ("Send comment", &[Action::SendComment]),
    ("Open in editor", &[Action::OpenInEditor]),
    ("Local mark", &[Action::LocalMark]),
    ("Pin or unpin", &[Action::Pin]),
    ("Pick a theme", &[Action::PickTheme]),
    ("Reload themes", &[Action::ReloadThemes]),
    ("Back / forward", &[Action::GoBack, Action::GoForward]),
    ("Open on GitHub", &[Action::OpenOnGithub]),
    ("Switch Repository", &[Action::SwitchRepository]),
    ("Reset the layout", &[Action::ResetLayout]),
    (
        "Archive / Mute the selected item",
        &[Action::Archive, Action::Mute],
    ),
    (
        "Mark seen / unseen, the row under the cursor or the Detail pane's item, by focus",
        &[Action::MarkSeen, Action::MarkUnseen],
    ),
    ("Settings", &[Action::OpenSettings]),
    (
        "Zoom in / out / reset",
        &[Action::ZoomIn, Action::ZoomOut, Action::ZoomReset],
    ),
    ("Show or hide the performance HUD", &[Action::ToggleHud]),
    (
        "Open DevTools, in a development build only",
        &[Action::OpenDevtools],
    ),
];

/// A key as `docs/devel/keyboard.md` writes it, "Ctrl+K O" or "Alt+←", named as the table names it. It
/// reads as a line of `[keys]` would.
fn name_of(label: &str) -> String {
    keys(label).map(|key| key.to_string()).unwrap_or_default()
}

/// Each row of `docs/devel/keyboard.md`'s table: its Action cell, and the keys between backticks in its Key
/// cell.
fn rows_in_the_docs() -> Vec<(String, Vec<String>)> {
    let page = include_str!("../../../../docs/devel/keyboard.md");
    page.lines()
        .skip_while(|line| !line.starts_with("| Action"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let keys = cells[1]
                .split('`')
                .skip(1)
                .step_by(2)
                .map(name_of)
                .collect();
            (cells[0].to_owned(), keys)
        })
        .collect()
}

#[test]
fn each_action_has_the_key_the_docs_give_it() {
    let rows = rows_in_the_docs();
    assert_eq!(
        rows.len(),
        DOCUMENTED.len(),
        "keyboard.md's table has {rows:?}"
    );
    for (action_cell, keys) in rows {
        let actions = DOCUMENTED
            .iter()
            .find(|(cell, _)| *cell == action_cell)
            .map(|(_, actions)| *actions)
            .unwrap_or_else(|| panic!("keyboard.md's row {action_cell:?} names no action here"));
        let table_keys: Vec<String> = actions
            .iter()
            .filter_map(|action| TABLE.iter().find(|row| row.action == *action))
            .filter_map(|row| row.key.map(|key| key.to_string()))
            .collect();
        assert_eq!(table_keys, keys, "keyboard.md's row {action_cell:?}");
    }
}

/// Every action is in keyboard.md's table but `?`, which its text names under the table.
#[test]
fn the_docs_list_every_action() {
    for row in TABLE {
        let listed = DOCUMENTED
            .iter()
            .any(|(_, actions)| actions.contains(&row.action));
        assert!(
            listed || row.action == Action::ShowKeys,
            "keyboard.md lacks {:?}",
            row.action
        );
    }
}
