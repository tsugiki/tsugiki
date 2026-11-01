# One SQLite database, and a mirror per Repository

Everything Tsugiki keeps about GitHub, and everything the user keeps in Tsugiki, is in one SQLite file, `tsugiki.db`, accessed with `rusqlite` (SQLite bundled into the app). It covers every Repository: the cache (Issues, PRs, reviews, threads and comments with GitHub's `bodyHTML`, CI checks, timeline events, the facts that link Issues, PRs and Backports ([ADR 0011](0011-pr-links-from-github-signals-by-repository-conventions.md)), and per-file patch-ids keyed by commit) and the user's own state (Pinned issues, Muted authors, Local marks, Last seen, Archived and Muted items, unsent composer text; settings stay in TOML files, [ADR 0014](0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)). Each Repository's mirror is a folder of its own ([ADR 0002](0002-tsugiki-owns-its-git-state.md)):

```
%LOCALAPPDATA%\Tsugiki\              ~/Library/Application Support/Tsugiki/ on macOS
  tsugiki.db
  repos\python__cpython\mirror.git\
```

In portable mode all of it lives in a `data` folder next to the program instead ([ADR 0014](0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)).

One file is the unit the user moves to another machine. **Export database** writes a consistent snapshot of it with `VACUUM INTO`, and **Import database** replaces the other machine's file with that snapshot, after a confirmation and with a backup kept. The cache travels too, which saves a new machine CPython's first sync of 8 to 10 minutes. The mirrors are cloned again there; with `--reference` that takes under a minute.

The database stores facts, as GitHub and git report them. Needs attention, review state, Issue trees and counts are computed in Rust when a screen asks ([ADR 0008](0008-view-models-and-invalidation-over-ipc.md)), so they can't go stale or miss a rule change. The one exception is work too slow to repeat: per-file patch-ids, keyed by commit, which can't change. Anything else is stored only if profiling on CPython asks for it.

## Considered Options

- **A separate `state.db`, plus a cache next to each mirror.** It keeps data that can be thrown away apart from data that can't. But two kinds of file would have to be moved between machines, and the user's state is small enough to keep safe within one file (see Consequences).
- **sqlx.** Its SQL is checked at compile time, but it needs a database or prepared query metadata at build time, and async adds nothing for a local file. **A key-value store such as redb** would make every query across PRs hand-built.
- **Storing derived state**, or **raw GraphQL JSON**. The first needs a recompute pass on every rule change, and a missed one shows wrong badges. The second parses blobs on every query.
- **Syncing the live file with OneDrive or Dropbox.** Unsafe while the app runs: WAL keeps recent writes in a separate `-wal` file, and a sync tool copies the two at different moments.

## Consequences

- Cache tables and state tables are kept apart in the schema. Resetting a Repository's cache deletes only its cache rows, in one transaction. Removing a Repository deletes its rows and its mirror folder.
- SQLite runs in WAL mode on a dedicated thread, so screen queries never wait on a sync, and each sync step is written in one transaction.
- The schema version is SQLite's `user_version`. Numbered migrations run at start-up in one transaction, after Tsugiki writes a backup snapshot (`tsugiki.db.v<N>.bak`, keeping the last few). State tables are always migrated, never dropped. Cache tables may be dropped and synced again when migrating them isn't worth it, and the release notes say so. An older Tsugiki that finds a newer file refuses to start and says why, without touching it.
- Import replaces the whole file; two machines' changes are not merged.
