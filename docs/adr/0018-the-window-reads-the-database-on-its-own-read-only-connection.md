# The window reads the database on its own read-only connection

A first sync on CPython keeps the session thread busy with GitHub for minutes, and a stuck request holds it
for up to 15 s. The window still has to draw its lists in that time:
[ADR 0017](0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md) gives a view
query 50 ms, and GitHub being down can't block the UI.

So the database has two connections, each on a thread of its own. `Store` is the only writer, and only the
session holds it. `StoreReader` is a read-only connection that the app's commands share. In WAL mode
([ADR 0010](0010-one-sqlite-database-and-per-repository-mirrors.md)) it can read while the session saves a
batch. It reads each view's facts in one transaction, so a view never mixes two saves.

The session opens the reader after `Store` has run the migrations, and a question that arrives earlier waits
for that. `StoreReader` holds only a weak link to its thread, so the session alone decides when that thread
ends. Closing both connections on quit removes the `-wal` file.

## Considered Options

- **Reads as messages to the session thread.** One thread would own all the data, which is the simplest rule.
  A read would then wait for the request to GitHub in flight, and that breaks the 50 ms budget on every list.
- **Reads on `Store`'s thread, with the session sending it writes.** The window would stop waiting on GitHub,
  but each read would wait for the save ahead of it.
- **A connection for each command.** Nothing needs reads to run side by side yet, and each command would pay
  to open SQLite again.

## Consequences

- Reads run one at a time on the reader's thread, so a slow view delays the next one. A pool can replace the
  thread behind the same type if a measurement asks for it.
- Commands that read are `#[tauri::command(async)]`, so they wait away from the thread that draws the window.
- The window can read just before a save it then hears about. It asks again after each event and keeps the
  newest answer, so it ends on the latest facts.
