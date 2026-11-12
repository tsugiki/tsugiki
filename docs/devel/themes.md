# Themes

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).
Decided in [#374](https://github.com/tsugiki/tsugiki/issues/374), with the prototype on `prototype/themes`.

- Every colour in the window comes from the **Theme** in force. The window follows Windows to the light or
  the dark side, and when Windows changes sides while Tsugiki runs, the window jumps to the other Theme with
  no fade. The root's `color-scheme` takes the Theme's kind, so scroll bars and form controls follow too.
- Two Themes are built in: VS Code Light Modern for the light side and VS Code Dark Modern for the dark side.
  The Sidebar and the status bar sit a step off the content, with a border, as in VS Code.
- A Theme is a TOML file: `name`, `kind` (`"light"` or `"dark"`), and `[colors]` and `[syntax]` tables of
  tokens. Colours are `#rgb`, `#rrggbb` or `#rrggbbaa`. An unknown key or a bad value skips its line and the
  rest still loads; a file that isn't TOML, or has no `name` or `kind`, can't be used. The built-in Themes
  are in `crates/core/themes/`, compiled in.
- The core parses the file and sends the window each token as a CSS variable on the root: `fg_muted` is
  `--fg-muted`, and a syntax token takes `hl-` in front, so `keyword` is `--hl-keyword`. Components read only
  these variables, apart from borders and tints mixed from `currentColor`.

## Tokens

A core test fails when this table and the built-in Themes disagree.

| Token | What it colours | Light Modern | Dark Modern |
|---|---|---|---|
| `bg` | The content's background: the Issue list and the Detail pane | `#ffffff` | `#1f1f1f` |
| `fg` | Text | `#3b3b3b` | `#cccccc` |
| `fg_muted` | Quieter text: notes, the HUD's headings | `#767676` | `#9d9d9d` |
| `border` | Borders between panes, around dialogs and inside them | `#e5e5e5` | `#2b2b2b` |
| `bg_chrome` | The Sidebar's and the status bar's background | `#f8f8f8` | `#181818` |
| `fg_chrome` | The Sidebar's and the status bar's text | `#3b3b3b` | `#cccccc` |
| `bg_overlay` | Dialogs, the palette and the performance HUD | `#f8f8f8` | `#222222` |
| `backdrop` | The scrim behind a dialog or the palette | `#00000026` | `#00000073` |
| `shadow` | The shadow under a dialog, the palette or the HUD | `#00000029` | `#00000099` |
| `selected` | The palette's selected row | `#e8e8e8` | `#04395e` |
| `accent` | Hunk headers and the diff's Show more rows, as a tint | `#005fb8` | `#4daafc` |
| `add` | Added lines, as a tint | `#48a14d` | `#2ea043` |
| `del` | Removed lines, as a tint | `#e51400` | `#f85149` |
| `find` | The search's matches in the diff, as a tint | `#f8c94a` | `#9e6a03` |
| `find_current` | The match the search is on | `#a8ac94` | `#515c6a` |
| `find_current_fg` | The text of the match the search is on | `#3b3b3b` | `#cccccc` |
| `attention` | The mark on a draft from the user's Pending review | `#bf8803` | `#cca700` |
| `danger` | Errors and missed budgets in the HUD | `#e51400` | `#f14c4c` |
| `warn_bg` | The status bar's Offline background | `#c72e0f` | `#c72e0f` |
| `warn_fg` | The status bar's Offline text | `#ffffff` | `#ffffff` |
| `state_open` | The icon of an open Issue or PR | `#1a7f37` | `#3fb950` |
| `state_done` | The icon of a merged PR or an Issue closed as done | `#8250df` | `#ab7df8` |
| `state_closed` | The icon of a PR closed unmerged | `#d1242f` | `#f85149` |
| `state_muted` | The icon of a Draft PR or an Issue closed as not planned | `#767676` | `#9d9d9d` |
| `keyword` | Keywords and operators in code, from `[syntax]` | `#0000ff` | `#569cd6` |
| `string` | Strings in code, and links in Markdown | `#a31515` | `#ce9178` |
| `comment` | Comments in code | `#008000` | `#6a9955` |
| `constant` | Numbers, constants and escapes in code, and Markdown's headings | `#098658` | `#b5cea8` |
| `entity` | Functions, attributes and types in code | `#795e26` | `#dcdcaa` |
| `tag` | Tags in HTML and XML | `#267f99` | `#4ec9b0` |
