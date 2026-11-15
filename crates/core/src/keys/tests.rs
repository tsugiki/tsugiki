use super::*;

fn binding(action: Action, key: Option<Keys>, context: Context) -> Binding {
    Binding {
        action,
        title: "",
        key,
        context,
    }
}

fn press(modifiers: Modifiers, key: &'static str) -> Press {
    Press { modifiers, key }
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
    let e = Some(Keys::One(press(Modifiers::Plain, "e")));
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
    let ctrl_enter = Some(Keys::One(press(Modifiers::Ctrl, "enter")));
    let send = binding(Action::SendComment, ctrl_enter, Context::Composer);
    let find = binding(Action::FindInDiff, ctrl_enter, Context::Diff);

    assert!(send.clashes(&find));
    assert!(find.clashes(&send));
}

#[test]
fn different_keys_never_clash() {
    let one = |key| Some(Keys::One(press(Modifiers::Plain, key)));
    let chord = |second| {
        Some(Keys::Chord(
            press(Modifiers::Ctrl, "k"),
            press(Modifiers::Plain, second),
        ))
    };

    assert!(
        !binding(Action::Archive, one("e"), Context::Anywhere).clashes(&binding(
            Action::Mute,
            one("m"),
            Context::Anywhere
        ))
    );
    assert!(
        !binding(Action::LocalMark, chord("m"), Context::Anywhere).clashes(&binding(
            Action::Pin,
            chord("p"),
            Context::Anywhere
        ))
    );
    assert!(
        binding(Action::LocalMark, chord("m"), Context::Anywhere).clashes(&binding(
            Action::Pin,
            chord("m"),
            Context::Anywhere
        ))
    );
}

#[test]
fn a_key_clashes_with_the_chords_it_starts() {
    let ctrl_k = binding(
        Action::Archive,
        Some(Keys::One(press(Modifiers::Ctrl, "k"))),
        Context::Anywhere,
    );
    let local_mark = binding(
        Action::LocalMark,
        Some(Keys::Chord(
            press(Modifiers::Ctrl, "k"),
            press(Modifiers::Plain, "m"),
        )),
        Context::Anywhere,
    );
    let ctrl_m = binding(
        Action::Mute,
        Some(Keys::One(press(Modifiers::Ctrl, "m"))),
        Context::Anywhere,
    );

    assert!(ctrl_k.clashes(&local_mark));
    assert!(local_mark.clashes(&ctrl_k));
    assert!(!ctrl_m.clashes(&local_mark));
}

#[test]
fn an_action_with_no_key_clashes_with_none() {
    let reset = binding(Action::ResetLayout, None, Context::Anywhere);
    let e = binding(
        Action::Archive,
        Some(Keys::One(press(Modifiers::Plain, "e"))),
        Context::Anywhere,
    );

    assert!(!reset.clashes(&e));
    assert!(!e.clashes(&reset));
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
    ("Show or hide the rail's Files", &[Action::ToggleRailFiles]),
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

/// A key as `docs/devel/keyboard.md` writes it, "Ctrl+K O" or "Alt+←", named as the table names it.
fn name_of(label: &str) -> String {
    let press = |press: &str| {
        press
            .split('+')
            .map(|part| match part {
                "←" => "arrowleft".to_owned(),
                "→" => "arrowright".to_owned(),
                _ => part.to_lowercase(),
            })
            .collect::<Vec<_>>()
            .join("+")
    };
    label.split(' ').map(press).collect::<Vec<_>>().join(" ")
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
