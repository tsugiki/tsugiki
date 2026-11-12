# Themes

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).
Decided in [#374](https://github.com/tsugiki/tsugiki/issues/374), with the prototype on `prototype/themes`.

- Every colour in the window comes from the **Theme** in force. By default the window follows Windows to the
  light or the dark side, and when Windows changes sides while Tsugiki runs, the window jumps to the other
  Theme with no fade. The root's `color-scheme` takes the Theme's kind, so scroll bars and form controls
  follow too.
- `settings.toml` picks the mode and one Theme for each side
  ([#517](https://github.com/tsugiki/tsugiki/issues/517)):

  ```toml
  [appearance]
  theme = "system"   # "system", "light" or "dark"
  light_theme = "VS Code Light Modern"
  dark_theme = "Gruvbox Dark Hard"
  ```

  System, the default, takes the side Windows is on. Light and Dark keep their side whatever Windows says,
  and turn the title bar and the native controls with it through Tauri's `set_theme`. A key left out takes
  its default: System, VS Code Light Modern and VS Code Dark Modern. A `theme` other than the three, or a
  name that isn't text, keeps that key's default, and the status bar names the file and the line. A name
  no Theme of that side has stays in the settings, as its file may be mended: until then the window shows
  the side's default, and the status bar says so. Tsugiki reads `settings.toml` at start, so a hand edit
  takes effect at the next one.
- `Ctrl+K T`, or Pick a Theme in the command palette, opens the theme picker in the palette, as VS Code's
  `Ctrl+K Ctrl+T` does ([#518](https://github.com/tsugiki/tsugiki/issues/518)). It lists the Themes of the
  side in use, opens on the one in force and marks it "chosen". Each Theme the list moves to shows on the
  whole window, with no fade. Enter saves the Theme picked to `light_theme` or `dark_theme` at once, as
  Tsugiki saves the panes' layout, without Settings' Apply. Esc or a click outside puts the Theme in force
  back, and so does moving to the last row. That row,
  "Dark themes…" on the light side or "Light themes…" on the dark side, lists the other side's Themes with
  that side's choice marked: a pick there saves that side's setting and leaves the mode alone, so the window
  goes back to the Theme in force. A save that fails puts the old choice back, and the status bar says why.
  The user's own Themes come after the built-in ones, in name order, and then each file that can't be used,
  greyed, with its file and why. Enter on it does nothing. Then comes "Reload themes", which reads the
  `themes` folder again and keeps the list open. `ui/src/lib/state/theme-picker.svelte.ts` decides what each
  row does.
- The window opens hidden, takes the `bg` of the Theme for the side Windows is on, and only then shows, so
  a dark Theme doesn't flash white while the page loads
  ([#516](https://github.com/tsugiki/tsugiki/issues/516)). The page's own background starts on the same
  colour until the Theme arrives, and the window takes the other side's `bg` when Windows changes sides.
  `app/src/first_frame.rs` does it. Four gaps remain:
  - Tauri reads high contrast as the light side, so a dark contrast theme still opens white.
  - On macOS Tauri can't colour the webview, so WKWebView paints white until the page loads.
  - The window opens before the core reads the settings, so it opens on the side Windows is on, and a Light
    or Dark mode turns it a moment later.
  - The window's own background is the `bg` of VS Code's Theme for its side, whichever Theme the settings
    pick. The page covers it, and it shows only at the edge of a resize.
- Four Themes are built in. VS Code Light Modern and Gruvbox Light Hard are for the light side, and VS Code
  Dark Modern and Gruvbox Dark Hard for the dark side. In VS Code's two, the Sidebar and the status bar sit a
  step off the content, with a border. The Gruvbox Themes take their colours from the palette in
  [morhetz/gruvbox](https://github.com/morhetz/gruvbox): the hard background, `bg0_h`, with the faded
  colours on the light side and the bright ones on the dark side.
- A Theme is a TOML file: `name`, `kind` (`"light"` or `"dark"`), and `[colors]` and `[syntax]` tables of
  tokens. Colours are `#rgb`, `#rrggbb` or `#rrggbbaa`. An unknown key or a bad value skips its line and the
  rest still loads; a file that isn't TOML, or has no `name` or `kind`, can't be used. The built-in Themes
  are in `crates/core/themes/`, compiled in.
- The user's own Themes are `.toml` files in a `themes` folder beside the settings file in use, so a
  `settings_path` synced from elsewhere brings its Themes along
  ([#519](https://github.com/tsugiki/tsugiki/issues/519)). Each joins its `kind`'s list, and `light_theme`
  and `dark_theme` can name it:

  ```toml
  name = "Solarized Dark"
  kind = "dark"
  extends = "VS Code Dark Modern"   # optional

  [colors]
  bg = "#002b36"
  fg = "#839496"

  [syntax]
  keyword = "#859900"
  ```

  A file sets only the tokens it changes. The rest come from the Theme `extends` names, a built-in one or
  another file, or else from its `kind`'s default: VS Code Light Modern or VS Code Dark Modern. A file of
  five colours then works, and a token added later gets a value in old files. An `extends` that loops, names
  no Theme, names a Theme of the other side, or names a file that can't be used fails the whole file. A
  file can't take a built-in Theme's name. When two files share a name, the first in file-name order keeps
  it, and the later one fails.
- Tsugiki reads the folder when it starts, when the picker opens, and on Reload themes: `Ctrl+K R`, the
  palette command or the picker's row. It has no watcher. A skipped line, or a file that isn't a Theme,
  shows in the status bar with its file and line, and a skipped line names its key too. So does a folder
  Tsugiki can't list; one that isn't there holds no Theme. A file that can't
  be used shows greyed in the picker. `ThemeLibrary::read` in `core/src/theme.rs` reads the folder.
- The core parses the file and sends the window each token as a CSS variable on the root: `fg_muted` is
  `--fg-muted`, and a syntax token takes `hl-` in front, so `keyword` is `--hl-keyword`. Components read only
  these variables, apart from borders and tints mixed from `currentColor`.

## Type and controls

`ui/src/lib/ui/window.css` holds these rules for every screen, from the prototype on `prototype/app-shell`
([#598](https://github.com/tsugiki/tsugiki/issues/598)). Its colours come from the tokens below.

- **Type.** The UI font is `system-ui, -apple-system, "Segoe UI", sans-serif` at 14 px. Code, SHAs and
  branches take `ui-monospace, "Cascadia Mono", "SF Mono", Consolas, monospace`: a `<code>` or `<pre>`
  does, and so does anything with the `mono` class. Text in the UI font inside code, such as a review
  thread in the diff, takes the `sans` class. Descriptions, comments and other long text take the
  `reading` class, a line height of 1.5. A key in `<kbd>` keeps the UI font. No component names a font
  family of its own, and a test fails on one that does.
- **Buttons.** A 1 px `border`, 4 px corners, `2px 8px` padding, `bg_chrome` behind, a tint of the text
  colour under the pointer, and half opacity when disabled. A button that reads as a link takes the `link`
  class: plain text, underlined under the pointer and with the focus.
- **Inputs.** Text inputs, text areas and selects take the same border and corners on `bg`, and an
  `accent` border with the focus.
- **Chips.** One pill for a review's or a thread's state (stale, outdated, resolved, approved, changes
  requested) and for a label: the `chip` class, with a 1 px border, round ends and 11 px text. A chip
  that sets a state's colour on its text takes it on its border too.
- **Muted text.** Ages, counts and hints take `fg_muted`, and don't fade with opacity.
- **Small headings.** A section's heading in the Sidebar, and the Issue list's quiet line, take the
  `small-heading` class: 11 px, `fg_muted`, upper case and 0.04 em apart.

The rules sit in `:where()`, which weighs nothing, so a component's own rule for a control wins over them.

## Tokens

A core test fails when this table and the built-in Themes disagree.

| Token | What it colours | Light Modern | Gruvbox Light Hard | Dark Modern | Gruvbox Dark Hard |
|---|---|---|---|---|---|
| `bg` | The content's background: the Issue list and the Detail pane | `#ffffff` | `#f9f5d7` | `#1f1f1f` | `#1d2021` |
| `fg` | Text | `#3b3b3b` | `#3c3836` | `#cccccc` | `#ebdbb2` |
| `fg_muted` | Quieter text: notes, the HUD's headings | `#767676` | `#7c6f64` | `#9d9d9d` | `#a89984` |
| `border` | Borders between panes, around dialogs and inside them | `#e5e5e5` | `#ebdbb2` | `#2b2b2b` | `#3c3836` |
| `bg_chrome` | The Sidebar's and the status bar's background, a comment's header, code in a description and Backports in the Issue view's tree | `#f8f8f8` | `#fbf1c7` | `#181818` | `#282828` |
| `fg_chrome` | The Sidebar's and the status bar's text | `#3b3b3b` | `#3c3836` | `#cccccc` | `#ebdbb2` |
| `bg_overlay` | Dialogs, the palette and the performance HUD | `#f8f8f8` | `#fbf1c7` | `#222222` | `#282828` |
| `backdrop` | The scrim behind a dialog or the palette | `#00000026` | `#00000026` | `#00000073` | `#00000073` |
| `shadow` | The shadow under a dialog, the palette or the HUD | `#00000029` | `#00000029` | `#00000099` | `#00000099` |
| `selected` | The palette's selected row and the Sidebar's open view | `#e8e8e8` | `#ebdbb2` | `#04395e` | `#504945` |
| `selection_active` | The Issue list's row under the cursor while the list holds the focus | `#e8e8e8` | `#ebdbb2` | `#04395e` | `#504945` |
| `selection_inactive` | That row while the focus is elsewhere | `#e4e6f1` | `#f2e5bc` | `#37373d` | `#3c3836` |
| `focus_border` | The line along the top of the pane that holds the focus | `#005fb8` | `#076678` | `#0078d4` | `#458588` |
| `accent` | The bar and the title of an item that Needs attention, focus borders, PR numbers, links and a Review request in the Detail pane, the open tab's line, and as a tint, hunk headers and the diff's Show more rows | `#005fb8` | `#076678` | `#4daafc` | `#83a598` |
| `add` | Added lines, as a tint | `#48a14d` | `#79740e` | `#2ea043` | `#b8bb26` |
| `del` | Removed lines, as a tint | `#e51400` | `#9d0006` | `#f85149` | `#fb4934` |
| `find` | The search's matches in the diff, as a tint | `#f8c94a` | `#d79921` | `#9e6a03` | `#d79921` |
| `find_current` | The match the search is on | `#a8ac94` | `#d5c4a1` | `#515c6a` | `#665c54` |
| `find_current_fg` | The text of the match the search is on | `#3b3b3b` | `#282828` | `#cccccc` | `#fbf1c7` |
| `attention` | The mark on a draft from the user's Pending review | `#bf8803` | `#b57614` | `#cca700` | `#fabd2f` |
| `danger` | Errors, missed budgets in the HUD, failed CI and a Review that requests changes | `#e51400` | `#9d0006` | `#f14c4c` | `#fb4934` |
| `warn_bg` | The status bar's Offline background | `#c72e0f` | `#af3a03` | `#c72e0f` | `#af3a03` |
| `warn_fg` | The status bar's Offline text | `#ffffff` | `#fbf1c7` | `#ffffff` | `#fbf1c7` |
| `state_open` | The icon of an open Issue or PR, CI that passed and an approving Review | `#1a7f37` | `#79740e` | `#3fb950` | `#b8bb26` |
| `state_done` | The icon of a merged PR or an Issue closed as done | `#8250df` | `#8f3f71` | `#ab7df8` | `#d3869b` |
| `state_closed` | The icon of a PR closed unmerged | `#d1242f` | `#9d0006` | `#f85149` | `#fb4934` |
| `state_muted` | The icon of a Draft PR or an Issue closed as not planned | `#767676` | `#928374` | `#9d9d9d` | `#928374` |
| `keyword` | Keywords and operators in code, from `[syntax]` | `#0000ff` | `#9d0006` | `#569cd6` | `#fb4934` |
| `string` | Strings in code, and links in Markdown | `#a31515` | `#79740e` | `#ce9178` | `#b8bb26` |
| `comment` | Comments in code | `#008000` | `#928374` | `#6a9955` | `#928374` |
| `constant` | Numbers, constants and escapes in code, and Markdown's headings | `#098658` | `#8f3f71` | `#b5cea8` | `#d3869b` |
| `entity` | Functions, attributes and types in code | `#795e26` | `#b57614` | `#dcdcaa` | `#fabd2f` |
| `tag` | Tags in HTML and XML | `#267f99` | `#427b58` | `#4ec9b0` | `#8ec07c` |
