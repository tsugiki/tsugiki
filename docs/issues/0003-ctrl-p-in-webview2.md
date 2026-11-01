# Check that Ctrl+P never opens Print in the Tauri window

Status: open

In the prototype, Ctrl+P and Ctrl+Shift+P opened WebView2's print dialog instead of the command palette. The main cause was the keyboard layout. On a Russian layout Ctrl+P arrives as `e.key = "з"`, so no binding matched and nothing called `preventDefault()`. Two smaller gaps: the Home page had no Ctrl+P binding, and nothing was prevented while Settings was open.

The fix is on branch `prototype/app-shell` (8e1af74):

- With a non-Latin layout, keys are named by their physical key (`e.code`), as in VS Code.
- A capture-phase guard always prevents the default for the keys the app owns: Ctrl+P, Ctrl+Shift+P, Ctrl+R, Ctrl+, and Alt+←/→.

It was tested only in headless Edge, by dispatching keys over CDP. A synthetic key there doesn't show whether WebView2's own Print accelerator still fires.

## To find out

In the Tauri window (`npm run proto`), with both a Latin and a Russian layout, press Ctrl+P and Ctrl+Shift+P:

- on the Home page;
- in a Repository;
- with Settings open;
- in a comment box.

The palette should open, or nothing should happen while Settings is open. Print must never open.

## Then

If Print still opens, the page can't prevent it. Turn off WebView2's browser accelerator keys instead (see [0004](0004-webview2-browser-accelerator-keys.md)).
