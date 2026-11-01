# WebView2's browser shortcuts are off; Tsugiki owns its keys

WebView2 has shortcuts of its own: Ctrl+P prints, Ctrl+R and F5 reload, Ctrl+F finds, Ctrl+± zooms, F12 and Ctrl+Shift+I open DevTools. Several clash with Tsugiki's keys, which follow VS Code: F12 is Go to definition, Ctrl+R switches Repository, Ctrl+P is Quick open. In the prototype, Ctrl+P opened the print dialog on a Russian keyboard layout, because the page never matched the key and so never prevented it.

So on Windows the release build turns the engine's shortcuts off when the window is created: `crates/app` reaches the WebView2 settings through Tauri's `with_webview` and calls `SetAreBrowserAcceleratorKeysEnabled(false)` with the `webview2-com` crate. The engine never sees those keys, whatever the page does. Tauri 2.12 doesn't expose the switch yet. An open pull request adds it to the window config (tauri-apps/tauri#16397), and when that is released Tsugiki uses it and drops its own code.

The page keeps its key handling from the prototype: keys are named by their physical key (`e.code`) on a non-Latin layout, as in VS Code, and a capture-phase guard prevents the default for every key Tsugiki owns. That still matters on macOS and in the browser-based tests ([ADR 0009](0009-testing-and-full-coverage.md)).

## Considered Options

- **The page guard alone.** No Rust code, but any key it doesn't list still reaches the engine, and whether `preventDefault()` stops WebView2's own Print accelerator was never shown in a real window.
- **Waiting for Tauri** to release the config option, with the page guard until then. The fix is a few lines now, and is easy to delete later.

## Consequences

- Debug builds keep the browser shortcuts, so DevTools opens there, F12 included.
- Zoom is Tsugiki's own: `Ctrl+=`, `Ctrl+-` and `Ctrl+0` (`Cmd` on macOS) and the palette's View: Zoom In, Zoom Out and Reset Zoom set the window's zoom through Tauri's `set_zoom`, in VS Code's steps. The level is kept per machine. Ctrl+wheel doesn't zoom, so a slip while scrolling a diff can't change it.
- Find is Tsugiki's own `Ctrl+F`, which the virtualized diff needs anyway, since the browser can't find rows that aren't rendered.
- On macOS, WKWebView has no browser shortcuts, but the menu bar sees keys first. Tauri's standard menu stays (Quit, Hide, Close Window, Edit, Window), with Tsugiki › Settings… added on `Cmd+,`, run by the same command as the key. A test fails if a default Tsugiki key equals a menu shortcut, and the Keyboard page refuses a remap onto one. Windows has no menu bar.
- The Windows smoke test "Ctrl+P never prints" ([ADR 0009](0009-testing-and-full-coverage.md)) checks this in the real window.
