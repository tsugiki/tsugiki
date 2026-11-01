# Decide how the app keeps WebView2's browser shortcuts away

Status: open

WebView2 has its own browser shortcuts, among them Ctrl+P (Print), Ctrl+R and F5 (Reload), Ctrl+F (Find), Ctrl+± (zoom), and F12 and Ctrl+Shift+I (DevTools). Several of them clash with gh-rev's keys: F12 is Go to definition, and Ctrl+R switches Repository. The setting that turns all of them off is `AreBrowserAcceleratorKeysEnabled`.

- wry, the library under Tauri, has the switch (`WebViewBuilderExtWindows::with_browser_accelerator_keys`).
- Tauri 2.12 doesn't pass it through, either in `tauri.conf.json` or on the window builder.

## Options

- **Set it at startup through `with_webview`**: get the controller's `ICoreWebView2Settings3` and call `SetAreBrowserAcceleratorKeysEnabled(false)`. This needs the `webview2-com` crate. It is the cleanest option: the engine never sees the keys. It also turns off DevTools keys and Ctrl+F in release builds, which a desktop app wants anyway; gh-rev would need its own Find in diff (`Ctrl+F` in scope.md).
- **Keep a page-side guard**, as the prototype does: a capture-phase handler prevents the default for every key the app owns. No Rust code is needed. But any key that isn't listed still reaches the engine, and it rests on [0003](0003-ctrl-p-in-webview2.md) holding.
- **Watch for Tauri exposing it**, and switch over then.

## Then

Choose one when the real app's key handling is built. If the choice is hard to reverse, write it down as an ADR.
