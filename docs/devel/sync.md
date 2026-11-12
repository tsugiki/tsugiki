# Sync

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Polling is tiered and every interval is configurable:
  - The Workbench is refreshed every few minutes, fetching only what has changed since the last poll.
  - An open Issue or PR is refreshed when it is opened, and about once a minute while it stays on screen.
  - Merged and closed PRs are fetched once and then frozen.
  - The user can always refresh by hand.
- The first sync of a Repository depends on its size ([ADR
  0015](../adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)). A small one (up to 500 open
  Issues and PRs) loads everything open, plus what closed in the last 30 days. A large one like CPython loads
  the last 30 days of updates; the threshold and the window are settings. Either way it loads what waits on
  the user first (Review requests, PRs with the user's Review, Own PRs, assignments, then the Filters), so the
  Workbench is right in about a minute, then the trees of the user's Issues, then the rest of the window
  newest first, with the PRs that changed in the window, and for a small one its open items from before the
  window. The status bar shows its progress. An interrupted sync resumes where it stopped. Widening the window
  loads only the added days; narrowing it deletes nothing.
- Older items come on demand: opening one fetches it and keeps it, a search in All issues also runs on GitHub,
  and All issues ends with "Load 30 more days".
- The local database is one file for all Repositories, holding both the cached GitHub data and the user's own
  state ([ADR 0010](../adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). **Export database**
  (Settings › Advanced, and the command palette) writes a consistent copy of it anywhere, such as a synced
  folder. **Import database** on another machine replaces that machine's database after a confirmation,
  keeping a backup, so Pinned issues, Muted authors, Local marks, Last seen, Archived and Muted items and the
  cache follow the user. The mirrors are cloned again on the new machine.
- Offline, Tsugiki is **Read-only**: it shows the mirror and the cached GitHub data. Writing anything needs
  the network. A token that can't write makes a Repository Read-only too: see [token.md](token.md).
- Tsugiki asks GitHub who is signed in about once a minute, and only that answer brings it back online.
  Refresh while offline asks at once: if GitHub answers, the sync, the mirror's fetches and the open item's
  reads carry on. If it still doesn't, Tsugiki stays offline and the sync waits for the next minute's ask.
