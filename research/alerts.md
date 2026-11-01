# Raising Alerts that open the right PR when clicked

Research for [#20](https://github.com/tsugiki/tsugiki/issues/20). Read on 2026-11-02 against `tauri` 2.12.5, `tauri-plugin-notification` 2.8.0 (released 2026-10-28; its source on crates.io matches plugins-workspace at [`74a44c1`](https://github.com/tauri-apps/plugins-workspace/tree/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/notification)), `notify-rust` 4.18.2, `tauri-winrt-notification` 0.8.1, `mac-notification-sys` 0.6.15, `user-notify` 0.4.2, `tauri-plugin-single-instance` 2.5.5 and `tauri-plugin-deep-link` 2.6.3. "Source" means read in that crate's code; "inferred" means reasoned from sources without a test; "measured" means tried on this Windows 11 machine. Nothing here was run on macOS.

## Answer

Yes on both platforms, but on Windows not through the notification plugin. The plugin reports a click on desktop (`Notification::on_action`, action `tap`), but on Windows only while the toast is on screen; from Notification Center or after a restart the click is lost, and it can't replace or remove a shown notification on either OS. On Windows the reliable way is a toast that launches a URL, `<toast launch="tsugiki://pr/…" activationType="protocol">`: Windows starts `tsg.exe tsugiki://pr/…`, and the single-instance plugin hands it to the running Tsugiki and lets it come to the front. That works from Notification Center and when Tsugiki is closed, needs no COM activator, and relies only on what the per-user NSIS installer already writes (a Start menu shortcut with AppUserModelID `io.github.tsugiki`, and an HKCU `tsugiki` URL scheme). The cost is a small toast module on the `windows` crate, since `tauri-winrt-notification` can't set `launch`, `activationType`, `Tag` or `Group`. On macOS the plugin's deprecated `NSUserNotification` path needs no permission request or Developer ID signature and reports clicks while Tsugiki runs, which is enough for v1; it can't replace, shows no banner while Tsugiki is the active app, and loses clicks after a restart. `UNUserNotificationCenter` removes those limits but needs a permission prompt and a signed bundle (ad-hoc may be enough; sources disagree), so it fits better with Developer ID signing. macOS keys notification permission by bundle ID, not code signature, so it should survive an ad-hoc-signed update (inferred).

## What `tauri-plugin-notification` 2.8.0 does on desktop

The desktop code is [`src/desktop.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/notification/src/desktop.rs). It wraps `notify-rust`, which uses `tauri-winrt-notification` on Windows and `mac-notification-sys` on macOS.

| Feature | Windows | macOS | Source |
|---|---|---|---|
| Click on the notification | Reported as action `tap` to `Notification::on_action` (Rust) and `onAction` (JS), only while the toast is on screen | Reported while Tsugiki runs, including from Notification Center | `show()` doc comment; [PR #3671](https://github.com/tauri-apps/plugins-workspace/pull/3671), CHANGELOG 2.6.0 |
| Action buttons | From a registered action type; only id and title are used | Two or more go in an "Options" menu; told apart by title | `show()`, `register_action_types()` |
| Payload on click | The whole `NotificationData` (id, title, body, `extra`) comes back in `ActionPerformed` | same | `imp::performed()` |
| Grouping, scheduling | Ignored: "the scheduling and grouping options are ignored" | same | `show()` doc comment |
| Replace or remove a shown one | No: `remove_active` "does nothing, `notify-rust` cannot remove a shown notification there" | same | `remove_active()` |
| Permission | `request_permission` always returns `Granted` without asking | same; the OS asks by itself on the first notification | `request_permission()`; [plugins-workspace#2143](https://github.com/tauri-apps/plugins-workspace/issues/2143) |
| Click after Tsugiki restarts | Lost | Lost | inferred: tracking lives in a per-process map |
| Under `tauri dev` | Uses PowerShell's AppUserModelID when the exe is in `target\debug` or `target\release`, so toasts say "Windows PowerShell" | Uses `com.apple.Terminal`, so they come from Terminal | `imp::Notification::build()`; Cargo.toml platform notes: "Only works for installed apps. Shows powershell name & icon in development." |

Each tracked notification on Windows and macOS gets a thread that blocks until the user acts on it (`imp::wait`). Clicks are only reported for notifications shown after the first `on_action` handler or JS listener was registered.

Why a Windows click is lost after the toast leaves the screen: `notify-rust`'s [`windows.rs`](https://docs.rs/crate/notify-rust/4.18.2/source/src/windows.rs) feeds both the toast's `Activated` and `Dismissed` events into one channel, and `wait_for_response` returns after the first event. When the banner times out into Notification Center, Windows raises `Dismissed` (`TimedOut`), the plugin's thread ends, and a later `Activated` has no receiver. The toast also has no `launch` attribute, so Windows has nothing to start either.

`tauri-winrt-notification` 0.8.1 ([`src/lib.rs`](https://docs.rs/crate/tauri-winrt-notification/0.8.1/source/src/lib.rs)) builds the toast XML itself. It writes no `launch` or `activationType` on `<toast>`, and buttons get only `content` and `arguments`. It sets `Tag` only for progress toasts and never sets `Group`. To use protocol activation or replacement, Tsugiki has to write its own toast code or patch this crate.

Open plugin issues that bear on this: [#3512](https://github.com/tauri-apps/plugins-workspace/issues/3512) (on Windows the JS `Notification.permission` is always `"denied"`; Tsugiki raises Alerts from Rust, so it doesn't matter here) and [#2143](https://github.com/tauri-apps/plugins-workspace/issues/2143) (macOS: notifications from release builds land in Notification Center without a banner; explained below).

## Windows

### What has to be registered

- **For a toast to show**, an unpackaged app needs an AppUserModelID that Windows knows. The documented ways are a Start menu shortcut with `System.AppUserModel.ID` set (and `System.AppUserModel.ToastActivatorCLSID` for COM activation), as in [Microsoft's COM activator walkthrough](https://learn.microsoft.com/en-us/windows/apps/develop/cpp-winrt/author-coclasses) and [Electron's notification guide](https://github.com/electron/electron/blob/main/docs/tutorial/notifications.md), or the Windows App SDK's `AppNotificationManager.Register()`, which "automatically sets up the COM server registration" for unpackaged apps ([console guide](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-console)). The SDK does this without a shortcut: it writes `HKCU\Software\Classes\AppUserModelId\<AUMID>` with `DisplayName`, `IconUri` and `CustomActivator` ([`AppNotificationUtility.cpp`](https://github.com/microsoft/WindowsAppSDK/blob/main/dev/AppNotifications/AppNotificationUtility.cpp)). That key is used by Microsoft's own code but not documented as a contract.
- **Tauri's NSIS installer already writes the shortcut.** [`installer.nsi`](https://github.com/tauri-apps/tauri/blob/31f29e197e7ac18797dbb9270fe658c1ca1010c8/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) creates `$SMPROGRAMS\Tsugiki.lnk` pointing at `tsg.exe` and runs `SetLnkAppUserModelId`, which sets `PKEY_AppUserModel_ID` to the bundle identifier `io.github.tsugiki` ([`utils.nsh`](https://github.com/tauri-apps/tauri/blob/31f29e197e7ac18797dbb9270fe658c1ca1010c8/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh)). For a per-user install, `$SMPROGRAMS` is the user's Start menu. No `ToastActivatorCLSID` is set. The shortcut is skipped when the installer runs with `/NS` or when the user ticks "Do not create shortcuts" on a Start menu page, and an update leaves the existing one in place. The uninstaller removes it and the AUMID's jump lists.
- **The plugin uses the same AUMID**: outside `target\debug` and `target\release` it calls `notification.app_id(identifier)`, which is `io.github.tsugiki`.
- **The URL scheme.** With `plugins.deep-link.desktop.schemes: ["tsugiki"]` in `tauri.conf.json`, the same installer writes `HKCU\Software\Classes\tsugiki` with `URL Protocol` and `shell\open\command` = `"…\tsg.exe" "%1"` (`SHCTX` is HKCU for a per-user install). At run time, `DeepLink::register("tsugiki")` writes the same keys pointing at the running exe ([deep-link `lib.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/deep-link/src/lib.rs)).

### How a click reaches the running Tsugiki

There are three mechanisms, and only the third suits Tsugiki without extra machinery.

1. **The in-process `Activated` event** (what the plugin uses). It fires only in the process that showed the toast, while that `ToastNotification` is being listened to. Neither the plugin nor `notify-rust` listens after the toast is dismissed, and nothing launches Tsugiki when it is closed.
2. **COM activation** (`ToastActivatorCLSID` on the shortcut plus a `LocalServer32` registration). This is what Microsoft documents for clicks "whether the notification is clicked in Action Center or not" ([author-coclasses](https://learn.microsoft.com/en-us/windows/apps/develop/cpp-winrt/author-coclasses)). Tsugiki would need a COM class implementing `INotificationActivationCallback` in `tsg.exe`, a GUID registered by the installer (an NSIS hook, since Tauri doesn't set one), and handling of the `-Embedding` launch. The Windows App SDK does all of this, but it means shipping or bootstrapping the App SDK runtime. That is too much for v1.
3. **Protocol activation.** The toast's `launch` string becomes a URL, and `activationType="protocol"` makes Windows "launch a different app using protocol activation" ([schema, ToastActivationType](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-schema)). Here the "different app" is `tsg.exe` itself, through the `tsugiki://` handler above. Windows runs `tsg.exe "tsugiki://pr/python/cpython/12345"`. The single-instance plugin's second process finds the first one's window, calls `AllowSetForegroundWindow(pid)` so the first may take focus ("Windows lets us bring a window to the front, but not the first instance. Hand that right over before we exit"), and sends its argv over `WM_COPYDATA` ([single-instance `windows.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/single-instance/src/platform_impl/windows.rs)). With its `deep-link` feature, the URL goes to the deep-link plugin's `on_open_url` before the single-instance callback. If Tsugiki isn't running, the same command simply starts it with the URL. `user-notify`, written for Delta Chat's Tauri port, takes this approach on Windows ([`windows.rs`](https://docs.rs/crate/user-notify/0.4.2/source/src/platform_impl/windows.rs): `launch="{launch_url}" activationType="protocol"`; its README says notifications "can be used to start your app" on Windows).

That a shell launch from a toast click may take the foreground is inferred, not tested: the user's click is the last input, and the launched `tsg.exe` passes the right on with `AllowSetForegroundWindow`.

### Replacing and removing

`ToastNotification.Tag` is "the unique identifier of this notification within the notification Group", at most 64 characters since Windows 10 1703 ([Tag](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.tag)). Sending a new toast with the same Tag and Group replaces the old one ([pending-update guide](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-pending-update)). `ToastNotificationManager.History.Remove(tag, group, aumid)` and `RemoveGroup` take toasts out of Notification Center ([manage notifications](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/manage-app-notifications)). A `<header>` element groups toasts under a title in Notification Center ([toast schema](https://learn.microsoft.com/en-us/uwp/schemas/tiles/toastschema/element-toast)). None of this is reachable through the plugin.

### Under `tauri dev`

- There is no shortcut for `io.github.tsugiki`. The plugin, and anything copying it, falls back to PowerShell's AUMID, so dev toasts are labelled "Windows PowerShell".
- Protocol activation works in dev if the dev build registers a scheme at run time. Registering `tsugiki` itself would point the installed copy's links at `target\debug\tsg.exe`, because the last registration wins. A dev-only scheme such as `tsugiki-dev` avoids that.
- Measured: `CreateToastNotifier("io.github.tsugiki.research-test").Show()` from Windows PowerShell 5.1, with no shortcut or registry key for that AUMID, did not fail, reported `Setting = Enabled` and left the toast in `History`. Whether a banner was drawn could not be told: the same capture with PowerShell's own AUMID showed no banner either, so something on this machine (probably Do Not Disturb or the full-screen terminal) was holding banners back. Treat "a toast for an unregistered AUMID shows" as unverified. Tauri's own note says it doesn't.

### Other Windows notes

- Toasts don't show for an elevated process ([quickstart note](https://github.com/MicrosoftDocs/windows-dev-docs/blob/docs/hub/apps/develop/notifications/app-notifications/app-notifications-quickstart.md)). Tsugiki never runs elevated.
- Do Not Disturb and Focus send toasts straight to Notification Center. A protocol-activated toast can still be clicked from there.

## macOS

### What the plugin does

`notify-rust` uses `mac-notification-sys`, which uses `NSUserNotificationCenter` unless the `preview-macos-un` feature is on ([`src/macos/mod.rs`](https://docs.rs/crate/notify-rust/4.18.2/source/src/macos/mod.rs)). The plugin doesn't turn that feature on. `NSUserNotificationCenter` is "deprecated at macOS 11.0: All NSUserNotifications API should be replaced with UserNotifications.frameworks API" ([Apple](https://developer.apple.com/documentation/foundation/nsusernotificationcenter)). It still delivers on macOS 26.5.1: an issue measured it in a signed bundle ([mac-notification-sys#95](https://github.com/h4llow3En/mac-notification-sys/issues/95)).

- **Identity.** `set_application(identifier)` hooks `NSBundle` to report `io.github.tsugiki`, but only if Launch Services knows an app with that bundle ID (`LSCopyApplicationURLsForBundleIdentifier`, [`objc/notify.m`](https://docs.rs/crate/mac-notification-sys/0.6.15/source/objc/notify.m)). After installing to /Applications and opening it once, it does. The plugin ignores a failure here, so an app run straight from the mounted `.dmg` may post under the wrong identity (inferred).
- **Permission.** `NSUserNotification` has no authorization call; macOS asks the user on the first notification. In #2143, users of release builds report the prompt and working notifications, while dev builds post as Terminal.
- **Clicks.** The crate's delegate handles `didActivateNotification:`. The plugin's thread waits until the notification is activated, dismissed, or gone from `deliveredNotifications`. A notification that moved from banner into Notification Center is still in `deliveredNotifications`, so a later click there is still reported while Tsugiki runs (inferred from the source). Clicking a notification activates the app; when the app wasn't running, macOS launches it and passes the notification in `NSApplication.launchUserNotificationUserInfoKey` ([Apple](https://developer.apple.com/documentation/appkit/nsapplication/launchusernotificationuserinfokey)). The plugin doesn't read that key, so a click that starts Tsugiki opens nothing.
- **No banner while Tsugiki is the active app.** The center "may suppress the notification if the application is already frontmost (the delegate can override this action)" ([Apple](https://developer.apple.com/documentation/foundation/nsusernotificationcenter)). The crate's delegate doesn't implement `shouldPresentNotification:` and takes the only delegate slot, so Tsugiki can't override it. #95 measured "banner drawn: no" whenever `NSApp.isActive`, even with the window closed or minimised. That probably explains #2143's "notifications only in Notification Center": they were sent from a button in the app. For Tsugiki, an Alert raised while Tsugiki is in front would go to Notification Center silently.
- **No replacing or removing**, as in the table above.

### `UNUserNotificationCenter` and ad-hoc signing

The modern API brings what the plugin lacks. A request with the same identifier as a delivered notification "replaces the old notification with the new one" ([Apple](https://developer.apple.com/documentation/usernotifications/unnotificationrequest/init(identifier:content:trigger:))). `threadIdentifier` groups notifications, and `removeDeliveredNotifications(withIdentifiers:)` removes them. A click arrives as `UNNotificationDefaultActionIdentifier` in `userNotificationCenter(_:didReceive:withCompletionHandler:)`, also when it launched the app, provided the delegate is set "before your app finishes launching" ([Apple](https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/delegate)). `willPresent` decides whether a banner shows while the app is in front.

What it costs:

- An explicit `requestAuthorization`. The system prompts once and "records that response" ([Apple](https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications)).
- A real `.app` bundle. It doesn't work under `tauri dev`, which runs a bare binary; `user-notify` returns `NoBundleId` there and suggests a mock in debug builds.
- A code signature. Apple's pages don't state the rule. Third-party reports agree that unsigned executables are refused ("Notifications are not allowed for this application", `UNErrorDomain` 1), and [desktop-notifier's docs](https://desktop-notifier.readthedocs.io/) say "An ad-hoc signature will be sufficient". [`user-notify`'s README](https://docs.rs/crate/user-notify/0.4.2/source/README.md) says macOS needs a signed bundle and "an Apple developer account to sign it". The sources disagree, and Tsugiki's ad-hoc `.dmg` was not tested. Electron's docs say the same: macOS notifications need the app to be code-signed ([Electron](https://github.com/electron/electron/blob/main/docs/tutorial/notifications.md)).

### Does permission survive an update with a new signature?

Probably yes (inferred, not tested). macOS's notification settings are identified by bundle ID alone. The MDM payload that manages them, `NotificationSettingsItem`, takes only a required `BundleIdentifier` ([Apple](https://developer.apple.com/documentation/devicemanagement/notifications/notificationsettingsitem)). Privacy (TCC) grants, by contrast, require a `CodeRequirement` ([PPPC Identity](https://developer.apple.com/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary/identity)), and the keychain checks the signature too, which is why ADR 0012 expects a keychain prompt after each update. Notifications aren't a TCC service. The macOS release checklist should still check it once: grant, install the next build over it, and send an Alert.

## Alternatives and their cost

| Option | Click from Notification Center / after restart | Replace / remove | Cost |
|---|---|---|---|
| `tauri-plugin-notification` (both OSes) | Windows: on screen only. macOS: from Notification Center while running; not after restart | No | Least code. Windows clicks are unreliable |
| Own WinRT toast code (`windows` crate: `ToastNotificationManager`, `XmlDocument`) with `launch` + `activationType="protocol"` | Yes, both; Windows starts `tsg` | Yes: `Tag`, `Group`, `History.Remove` | About 150–250 lines on Windows, plus deep-link (scheme) and single-instance's `deep-link` feature. Needs the installer's Start menu shortcut |
| Patch or fork `tauri-winrt-notification` to add `launch`, `activationType`, `Tag`, `Group` | as above | as above | Small upstream PR; then still not through `notify-rust` or the plugin, which don't expose these |
| `user-notify` 0.4.2 (Windows protocol + Tag/Group; macOS `UNUserNotificationCenter`) | Yes, both | Yes | One crate for both OSes, but young and single-maintainer, async on Tokio, no buttons on Windows. macOS signing risk as above |
| `notify-rust` with `preview-macos-un` | In-process | Partly | "Preview" API ("api not settled, reconsider in 5.0"); same signing needs |
| `mac-notification-sys` directly | Same as the plugin | No | Nothing gained over the plugin |
| COM activator (`ToastActivatorCLSID`) or the Windows App SDK | Yes | Yes | COM server in `tsg.exe`, installer GUID registration, or the App SDK runtime. Too heavy for v1 |
| `win32_notif` 0.15 | Exposes `activationType` on buttons and `Tag`/`Group` | Yes | Another young crate; no advantage over the `windows` crate for one toast template |

## Recommendation for #21

1. **Click opens the PR, through a URL.** Each Alert carries a target such as `tsugiki://pr/python/cpython/12345` (for an Issue, `…/issue/…`). On Windows it is the toast's `launch` with `activationType="protocol"`, and the scheme is registered by the installer (`plugins.deep-link.desktop.schemes`). On macOS it rides in the notification's `extra` data and comes back through `on_action`. Both end in one handler in the core that opens the PR and focuses the window. A deep link typed in a browser then works too, which Tsugiki may want anyway.
2. **Windows: own toast code, not the plugin.** Use `windows`-crate WinRT with AUMID `io.github.tsugiki`. Set `Tag` = the PR key and `Group` = the Alert kind, so a second CI result for the same PR replaces the first, and opening the PR in Tsugiki can `History.Remove` its toast. Under `tauri dev`, fall back to PowerShell's AUMID and a `tsugiki-dev` scheme. The README should say that installing with `/NS`, or without the Start menu shortcut, turns Alerts off.
3. **macOS: the plugin is enough for v1.** Accept no replacing, no banner while Tsugiki is in front, and no click after a restart. Move to `UNUserNotificationCenter` when Developer ID signing arrives (ADR 0012 puts it off, not rejects it). That move also removes the doubt about ad-hoc signing.
4. **Hold back Alerts while a Tsugiki window is focused**, on both OSes. macOS already does so for banners, and Windows doesn't, so Tsugiki has to decide this itself to behave the same. The New activity the Alert stands for is visible in the app anyway.
5. **Don't promise "click it tomorrow" on macOS.** A click on an old Alert after a restart opens nothing in v1. The New activity marker in the app stays the reliable record.
6. **Add to the macOS release checklist:** first Alert prompts for permission; a click opens the right PR from a banner and from Notification Center; permission survives installing the next build.

## Sources

Tauri and plugins:
- `tauri-plugin-notification` 2.8.0: [`src/desktop.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/notification/src/desktop.rs), [`Cargo.toml`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/notification/Cargo.toml), [`CHANGELOG.md`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/notification/CHANGELOG.md), [PR #3671](https://github.com/tauri-apps/plugins-workspace/pull/3671)
- Issues: [plugins-workspace#2143](https://github.com/tauri-apps/plugins-workspace/issues/2143), [#3512](https://github.com/tauri-apps/plugins-workspace/issues/3512)
- `tauri-plugin-single-instance`: [`platform_impl/windows.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/single-instance/src/platform_impl/windows.rs), [`lib.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/single-instance/src/lib.rs)
- `tauri-plugin-deep-link`: [`lib.rs`](https://github.com/tauri-apps/plugins-workspace/blob/74a44c1d5694324b7fb9ba5ee9271a3096da9387/plugins/deep-link/src/lib.rs)
- Tauri bundler NSIS template: [`installer.nsi`](https://github.com/tauri-apps/tauri/blob/31f29e197e7ac18797dbb9270fe658c1ca1010c8/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi), [`utils.nsh`](https://github.com/tauri-apps/tauri/blob/31f29e197e7ac18797dbb9270fe658c1ca1010c8/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh)

Crates:
- `notify-rust` 4.18.2: [`src/windows.rs`](https://docs.rs/crate/notify-rust/4.18.2/source/src/windows.rs), [`src/macos/mod.rs`](https://docs.rs/crate/notify-rust/4.18.2/source/src/macos/mod.rs), [`src/macos/unusernotifications.rs`](https://docs.rs/crate/notify-rust/4.18.2/source/src/macos/unusernotifications.rs)
- `tauri-winrt-notification` 0.8.1: [`src/lib.rs`](https://docs.rs/crate/tauri-winrt-notification/0.8.1/source/src/lib.rs)
- `mac-notification-sys` 0.6.15: [`objc/notify.m`](https://docs.rs/crate/mac-notification-sys/0.6.15/source/objc/notify.m); issue [#95](https://github.com/h4llow3En/mac-notification-sys/issues/95)
- `user-notify` 0.4.2: [README](https://docs.rs/crate/user-notify/0.4.2/source/README.md), [`platform_impl/windows.rs`](https://docs.rs/crate/user-notify/0.4.2/source/src/platform_impl/windows.rs)
- `win32_notif` 0.15.3: [source](https://docs.rs/crate/win32_notif/0.15.3/source/)

Microsoft:
- [toast element (`launch`)](https://learn.microsoft.com/en-us/uwp/schemas/tiles/toastschema/element-toast)
- [App notification schema (`ToastActivationType.Protocol`)](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-schema)
- [ToastNotification.Tag](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.tag)
- [Remove app notifications (Tag, Group)](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/manage-app-notifications)
- [Pending update (replace by Tag and Group)](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-pending-update)
- [App notifications with a console app (unpackaged `Register()`)](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-console)
- [Author COM components with C++/WinRT (toast COM activator, shortcut, `LocalServer32`)](https://learn.microsoft.com/en-us/windows/apps/develop/cpp-winrt/author-coclasses)
- [Windows App SDK `AppNotificationUtility.cpp`](https://github.com/microsoft/WindowsAppSDK/blob/main/dev/AppNotifications/AppNotificationUtility.cpp)

Apple:
- [NSUserNotificationCenter](https://developer.apple.com/documentation/foundation/nsusernotificationcenter), [`launchUserNotificationUserInfoKey`](https://developer.apple.com/documentation/appkit/nsapplication/launchusernotificationuserinfokey)
- [Asking permission to use notifications](https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications), [UNUserNotificationCenter.delegate](https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/delegate), [UNNotificationDefaultActionIdentifier](https://developer.apple.com/documentation/usernotifications/unnotificationdefaultactionidentifier), [UNNotificationRequest init (replacing by identifier)](https://developer.apple.com/documentation/usernotifications/unnotificationrequest/init(identifier:content:trigger:))
- [MDM NotificationSettingsItem](https://developer.apple.com/documentation/devicemanagement/notifications/notificationsettingsitem), [MDM PPPC Identity](https://developer.apple.com/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary/identity)

Secondary, for the signing question only:
- [Electron: Notifications](https://github.com/electron/electron/blob/main/docs/tutorial/notifications.md)
- [desktop-notifier documentation](https://desktop-notifier.readthedocs.io/)
