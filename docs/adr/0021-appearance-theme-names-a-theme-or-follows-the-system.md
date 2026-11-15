# `[appearance] theme` names a Theme, or follows the system

#517 gave `[appearance] theme` three modes: `"system"`, `"light"` and `"dark"`. Light and Dark kept the
window on one side, and `light_theme` and `dark_theme` said which **Theme** each side showed. The theme
picker saved to the side's key, so a pick from the other side's list changed nothing on screen, and the only
way to keep the window on one side was a hand edit and a restart. VS Code lets the user pick any Theme at any
moment, and keeps following the system as a choice of its own.

So `theme` holds `"system"` or the name of a Theme (#755):

```toml
[appearance]
theme = "Gruvbox Light Hard"   # or "system", the default
light_theme = "VS Code Light Modern"
dark_theme = "Gruvbox Dark Hard"
```

`"system"` shows `light_theme` or `dark_theme`, whichever side the system is on. A Theme's name shows that
Theme whatever the system says, and turns the title bar and the native controls to its `kind`. `light_theme`
and `dark_theme` say only what `"system"` shows. In the core, `ThemeChoice` holds a variant for System and
one that holds a `ThemeName`, in place of #517's `ThemeMode`, and a third for a mode a file written before
holds, until a save writes the name in its place.

## Considered Options

- **Keep the modes and add a key for a fixed Theme.** VS Code spreads the choice over
  `workbench.colorTheme` and `window.autoDetectColorScheme`, and a file can then follow the system and name a
  Theme at once, with one key overriding the other. One key can't disagree with itself.
- **Keep the modes, and have the picker set the mode as well as the side's key.** A pick from the dark list
  while the system is light would save Dark and `dark_theme`. The window shows what was picked, but the user
  who wants one Theme still reads it from two keys, and following the system is no longer a choice the picker
  can offer beside the Themes.

## Consequences

- `"light"` and `"dark"`, from files written before, show the Theme `light_theme` or `dark_theme` names,
  and the next save writes that name: an Apply of the Settings dialog, or a pick that saves the side's key,
  which writes the name the window then shows. The core keeps such a mode apart from a name until then, so a
  pick for its side shows at once, as it did before. A Theme can't take the names `system`, `light` or
  `dark`, as it can't take a built-in Theme's name, so `theme` never reads one name two ways.
- A name no Theme has stays in the file, as its Theme file may be mended. The window follows the system until
  then, and the status bar names the file and the line.
- At start the app turns the window to the side of a mode or a built-in Theme, which it knows without the
  `themes` folder, before the page loads. A Theme from the folder waits for the `themes` command, which
  reads the folder, decides the Theme in force from the settings it reads after the folder, and turns the
  window that asked to that Theme's side through Tauri's `set_theme`. Of two asks that cross, the one that
  finishes last read the settings last, so the title bar and the page end on the same answer.
- On Windows `set_theme` also keeps the webview's `prefers-color-scheme` on the side kept. So when the window
  follows the system again, the `themes` command says which side the window is on, and the page takes it
  before its media query hears of the change.
- The theme picker still saves to the side's key: #756 makes it list every Theme and Follow system, and save
  `theme`. Until then, a pick while `theme` names a Theme saves the side's choice and leaves the window on the
  named Theme.
