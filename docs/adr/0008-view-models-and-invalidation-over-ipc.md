# The webview gets view models and re-queries on change

Rust computes everything the screens show, and the webview only renders it. Each screen asks the core one query that returns a view model, ready to render: `workbench(repo, view)` gives the Issue list rows, `issue(repo, n)` the Issue view with its Issue tree, `pr(repo, n)` the PR view, and `sidebar(repo)` the views and their counts. Diffs keep the row-range commands from [ADR 0001](0001-standalone-tauri-gui-with-in-app-review.md). When a background refresh changes data, the core emits one `changed` event that names the Repository and the Issues and PRs it touched, and each open screen that shows any of them runs its query again. Selection, scroll position and composer text live in the webview, so a re-query doesn't disturb them.

This keeps every rule (Needs attention, review state, Issue trees, counts) in one place, where it is tested ([ADR 0007](0007-core-library-and-thin-tauri-app.md)). Re-running a query against the local store costs little, and a screen can't show a mix of old and new data.

## Considered Options

- **Entities with derived flags**, joined into screens by the webview. More flexible for a new screen, but the joining and counting would creep back into TypeScript.
- **Pushing patches** to subscribed screens. Fewer bytes, but every view model would need diffing and subscriptions in Rust, which is a lot of code for a local app.
- **Polling from the webview.** No event plumbing, but wasted work, and screens lag behind the status bar's "Synced".

## Consequences

- The TypeScript types and the functions for commands and events are generated from Rust (specta with tauri-specta, or `ts-rs` with a thin hand-written `invoke` layer if tauri-specta isn't ready). CI fails when the generated bindings are out of date.
- Commands return `Result<T, Error>`. `Error` is `{ kind, message }`, where the kind is one the UI acts on (`offline`, `unauthorized`, `write_refused`, `rate_limited` with its reset time, `not_found`, `conflict`, `internal`) and the message is in plain words. Details go only to a local log with tokens scrubbed ([ADR 0005](0005-token-stays-in-the-rust-core.md)).
- The core owns background work and app-wide status: running jobs and their progress, online or offline, token problems, the rate limit, each Repository's last sync, and unsent **Pending reviews**. It emits a `status` event when any of these changes, with progress throttled to a few updates per second. A `status()` query returns the whole state when the window loads. The webview never runs a job itself; it asks for one, such as Refresh.
- A write returns only after GitHub has accepted it, and the result shows up through `changed`. Nothing is shown that GitHub didn't take. A mutation that answers without the object it should create, such as `addPullRequestReviewThread` returning a null thread for a line outside the diff, counts as refused (`write_refused`).
- Unsent composer text is saved by the core per thread or line as the user types, so a reload, a crash or a refused write doesn't lose it. It is cleared after a successful send.
