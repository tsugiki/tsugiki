# The macOS checklist

Run it on the draft release's `.dmg` before you publish, in step 3 of [Making a release](releasing.md),
once you take macOS on. Until then Windows comes first, and a release skips it. The Windows smoke test checks
the real window on CI, but WKWebView doesn't speak the Chrome DevTools Protocol, so on macOS you check it by
hand ([ADR 0009](adr/0009-testing-and-full-coverage.md)). You need an Apple silicon Mac with `gh` signed in,
and about 15 minutes.

Note each check that fails. Publish only once it's fixed, or once it has an issue and you've chosen to ship
with it.

## Before you start

- Quit any Tsugiki that's running.
- If an earlier Tsugiki left `~/Library/Application Support/tsugiki`, move that folder aside so Tsugiki
  starts as a first install does. Put it back at the end.

## Install

1. Download the `.dmg` from the draft. To check it first, run `gh attestation verify` as
   [Checking a download](../README.md#checking-a-download) says.
2. Open the `.dmg` and drag Tsugiki to Applications.
3. Open Tsugiki from Applications. macOS blocks it. Open System Settings › Privacy & Security, click Open
   Anyway beside Tsugiki, and confirm. The window opens, and what you saw matches the steps in
   [Installing](../README.md#installing).
4. Quit Tsugiki and open it again. It opens with no warning.

## The window and its keys

On macOS, `Cmd` replaces `Ctrl` in Tsugiki's keys.

5. The window shows Onboarding. The status bar says `Connecting…`, then your login.
6. `Cmd+Shift+P` opens the palette, and `Esc` closes it. Do this check before the next two, which would also
   pass if no key reached the window.
7. `Cmd+P` opens no Print dialog.
8. Type a few letters in the box for a Repository's name, then press `Cmd+R`. The letters stay, so the page
   didn't reload.

## A Repository

9. Add `python/cpython`. The status bar shows the first sync's progress, and the Workbench fills in within
   about a minute.
10. `↑` and `↓` move through the Workbench, `Enter` opens an Issue, and `Cmd+Shift+G` opens that Issue in your
    browser.
11. Turn Wi-Fi off and click the sync time at the right of the status bar. The status bar says
    `<your login> · Offline: read-only`. Turn Wi-Fi on and click it again: your login comes back.
12. With CPython loaded, measure the performance budgets with `Cmd+Shift+H`, the other part of step 3 of
    [Making a release](releasing.md).

## One Tsugiki

13. With Tsugiki open, run this in Terminal:

    ```
    /Applications/Tsugiki.app/Contents/MacOS/tsg python/cpython#1
    ```

    The command returns, no second window opens, and the open one comes to the front.
    `~/Library/Application Support/tsugiki/tsugiki.log` gets the line
    `Asked to open the argument "python/cpython#1"`.
14. Run `open "tsugiki://pr/python/cpython/1"`. Tsugiki comes to the front, and the log gets
    `Asked to open the link "tsugiki://pr/python/cpython/1"`. Opening the item itself comes with Alerts.
15. Quit with `Cmd+Q`. `~/Library/Application Support/tsugiki` holds `tsugiki.db` and no `tsugiki.db-wal` or
    `tsugiki.db-shm`, so Tsugiki closed its database.

## Portable mode

16. Make a new folder, copy `Tsugiki.app` from Applications into it with Finder, and make an empty
    `tsugiki-portable-data` folder beside it. Open that copy, with Open Anyway if macOS blocks it. It starts
    on Onboarding, and `tsugiki-portable-data` gets `tsugiki.db` and `tsugiki.log`. If the data lands in
    `~/Library/Application Support/tsugiki` instead, macOS ran the copy from a hidden read-only place (App
    Translocation): note it as a failure. Quit, and delete the new folder.

## Checks that come later

- Alerts, in stage 4, add three checks ([ADR
  0016](adr/0016-alerts-launch-tsugiki-links-own-toasts-on-windows-the-plugin-on-macos.md)). The first Alert
  asks for permission. A click opens the right PR from a banner and from Notification Center. Permission
  survives installing the next build.
- The menu's Tsugiki › Settings… on `Cmd+,`, in stage 5
  ([ADR 0013](adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)).
