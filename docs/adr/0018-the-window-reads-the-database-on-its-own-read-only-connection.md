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

One Tsugiki at a time holds `Store` for a data folder. Before it reads or migrates the file, `Store` takes
an OS lock on `tsugiki.db.lock` beside it, and a second Tsugiki on that folder stops with a Problem that says
why. The single-instance plugin hands a second `tsg` over to the first, but on macOS two started together can
both pass its check, and another user's Tsugiki hides it (#126). The lock belongs to the folder, so an
installed Tsugiki, a development build and a portable copy each hold their own, and the system lets go of it
when a process ends, crash or not.

## Considered Options

- **Reads as messages to the session thread.** One thread would own all the data, which is the simplest rule.
  A read would then wait for the request to GitHub in flight, and that breaks the 50 ms budget on every list.
- **Reads on `Store`'s thread, with the session sending it writes.** The window would stop waiting on GitHub,
  but each read would wait for the save ahead of it.
- **SQLite's exclusive locking mode** in place of the lock file. It would also keep the reader out.
- **A connection for each command.** Nothing needs reads to run side by side yet, and each command would pay
  to open SQLite again.

## Consequences

- Reads run one at a time on the reader's thread, so a slow view delays the next one. A pool can replace the
  thread behind the same type if a measurement asks for it.
- Commands that read are `#[tauri::command(async)]`, so they wait away from the thread that draws the window.
- The window can read before a save it then hears about. It asks again after each event and keeps the newest
  answer, so it ends on the latest facts.
