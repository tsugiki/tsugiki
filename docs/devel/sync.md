# Sync

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Polling is tiered and every interval is configurable:
  - The Workbench is refreshed every few minutes, fetching only what has changed since the last poll.
  - An open Issue or PR is refreshed when it is opened, and about once a minute while it stays on screen.
    It waits while a PR from its tree or a hidden window hides it, and one whose minute came round
    meanwhile is refreshed as it shows again.
  - A merged or closed PR is fetched once, and then only when a refresh's poll sees its `updatedAt` move,
    so a comment on it, such as a Bot's that a backport failed, still arrives. The poll saves each changed
    Issue the database holds, whatever its state.
  - Before it finishes, a refresh fetches each Issue that a PR the database holds links to and the
    database lacks, and saves it as any other Issue ([workbench.md](workbench.md), #980). A number
    GitHub refuses alone waits for the next refresh while the rest of its batch is saved, and a batch
    GitHub refuses as a whole waits too.
  - A sync that sees an Issue or a Standalone PR close or merge, open in the database and closed in
    GitHub's answer, archives it if **Reach** held it while it was open
    ([workbench.md](workbench.md)). The database keeps each closing it saves until the sync judges it,
    right after the step that saved it, so a closing outlives a crash and waits for the next step that
    saves. An item that reopens leaves Archived as the sync saves it. An Archived item stays in Reach, so
    the sync keeps reading it and its PRs (#919).
  - A refresh sees a new PR when it loads the PRs that changed since the last one. The database keeps as
    an opening each of those PRs it saves for the first time that names an Archived Issue under any
    Conventions. A PR that a read of an Issue's tree, an older batch or an Origin PR saves for the first time
    was there before, and opens nothing. The sync judges the openings as it judges closings: it links each
    PR that isn't a Backport to the Issues it fixes under the Repository's Conventions and brings each such
    Archived Issue back to the Inbox ([workbench.md](workbench.md), #823). GitHub's poll gives no time a PR
    opened, so an old PR the cache lacks that changes counts as one that opened.
  - A refresh counts the Pinned issues as the user's work, beside the Filters' finds: it fetches each one
    the database lacks, and the poll saves each one that changed and reads its tree (#488). The database
    keeps whether a sync has read each pin's tree. A sync reads the trees of the pins none has read, and
    marks them read as it finishes, so one interrupted reads them again, and a pin made after it took them
    waits for the next. A fetch that shows a pin's number names no Issue drops the pin.
  - The user can always refresh by hand.
- The first sync of a Repository depends on its size ([ADR
  0015](../adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)). A small one (up to 500 open
  Issues and PRs) loads everything open, plus what closed in the last 30 days. A large one like CPython loads
  the last 30 days of updates; the threshold and the window are settings. Either way it loads what waits on
  the user first (Review requests, PRs with the user's Review, Own PRs, assignments, then the Filters and the
  Pinned issues), so the Workbench is right in about a minute, then the trees of the user's Issues, Pinned
  issues included, whatever their age, then the rest of the window
  newest first, with the PRs that changed in the window, and for a small one its open items from before the
  window. The status bar shows its progress. An interrupted sync resumes where it stopped. A page saved whose
  place then fails to save reaches the screens on the retry, or, if the sync fails 10 times with no step
  working in between and leaves the queue, as it leaves (#1046). Failures offline, at the rate limit or on a
  wrong clock don't count. Widening the window loads only the added days; narrowing it deletes nothing.
- A first sync and each refresh ask GitHub how many Issues and PRs the Repository has open, and the database
  keeps the latest answer for the Repository's page in Settings ([settings.md](settings.md)). A refresh
  saves it after the Issues the poll changed. An answer that differs from the one kept tells that page
  alone, and no screen of the Repository asks again; the same answer again writes nothing and tells
  nothing. The page also hears when a sync of its Repository finishes, open or not.
- A GraphQL answer says what it cost, and Tsugiki sums those costs since the hour began, whichever job
  asked: a sync, opening an Issue or a PR, the checks, asking who is signed in, the size the Mirror worker
  asks before a clone. The sum starts again when
  GitHub's reset moves to the next hour, and lives in memory only. The status bar shows the points left and
  what Tsugiki spent of those used ([status-bar.md](status-bar.md)), until the reset by GitHub's clock.
  Some requests count nothing: those whose answer doesn't say its cost, such as a 502, a timeout or an
  answer with no data, and those whose answer comes from an hour that has already reset. Whatever they cost,
  the status bar puts down to other tools.
- Below a floor of 500 points left, or after GitHub refuses a request for its rate limit, Tsugiki sends
  GitHub nothing until the reset, whichever job asks: the sync, the read of an opened Issue or PR and of its
  tree, quick open's fetch, Add repository's check, the Mirror worker's size check, and asking who is signed
  in. Each ask answers at once.
  The status bar says "Sync paused until 14:00: GitHub's rate limit is low", or with no sync waiting,
  "GitHub's rate limit is low. Tsugiki can ask it again at 14:00". A secondary rate limit holds about a
  minute, not until the hour's reset. A read on screen fails as one offline does, and the next read after
  the reset tries again. Quick open offers the number in the browser in place of a fetch
  ([keyboard.md](keyboard.md)). Add repository's check says when the points come back. Refresh asks
  nothing and leaves the status as it is. A refresh's list check keeps its tag for after the reset, so it
  can still cost nothing then. The first request after the reset goes as before, and its answer says the new
  hour's points. A clone whose size check the gate holds waits for the reset.
- Older items come on demand: opening one fetches it and keeps it. A search typed in the **Inbox** also runs
  on GitHub, ahead of the sync's next step: Tsugiki reads at most ten light pages of a hundred results, its
  1,000 cap, and keeps the answer in the cache. A result the cache lacks shows as a placeholder, which fills
  in a batch of up to 50 as the user scrolls to it: Tsugiki fetches the item's facts, keeps them, and the
  Inbox draws their row. Placeholders the user never scrolls to cost no request, and a placeholder the core
  handled is fetched once; one it couldn't handle comes back for a later ask. The Inbox ends with "Load 30
  more days" once it has listed all the cache holds.
- The local database is one file for all Repositories, holding both the cached GitHub data and the user's own
  state ([ADR 0010](../adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). **Export database**
  (Settings › Advanced, and the command palette) writes a consistent copy of it anywhere, such as a synced
  folder. **Import database** on another machine replaces that machine's database after a confirmation,
  keeping a backup, so Pinned issues, Muted authors, Local marks, Last seen, Archived and Muted items and the
  cache follow the user. The mirrors are cloned again on the new machine.
- An update that changes how Tsugiki links PRs to Issues and Origin PRs opens the database at once, and the
  screens show the links the last version worked out. Tsugiki then works them out again a Repository at a
  time, the open one first and before the next sync step, and each screen of a Repository reads it again
  once its links are done. A refill that fails keeps the old links, and the next start tries again.
- **Offline**, Tsugiki shows the mirror and the cached GitHub data, and what waits on GitHub goes once it
  answers. Offline isn't **Read-only**, which only a token that can't write or the user's choice makes a
  Repository: see [token.md](token.md). Each Repository's state, which each write control reads, says both
  apart.
  - An Issue view or a PR view without what the item says, and an Issue view of an Issue Tsugiki doesn't
    hold, say why the read failed: while GitHub can't be reached, that it comes once GitHub answers, and
    after a not-found, that GitHub doesn't have it or the token can't see it. The words go once a later read
    saves the item.
  - Mark Seen and Mark Unseen keep working, as they write only to the database.
- Tsugiki asks GitHub who is signed in about once a minute, and only that answer brings it back online.
  Refresh while offline asks at once, and the status bar says Connecting… until GitHub answers or the ask
  fails. If GitHub answers, the sync, the mirror's fetches and the open item's reads carry on. If it still
  doesn't, Tsugiki goes back to offline and the sync waits for the next minute's ask. A click while the ask
  is still out adds no ask, and a click once the status bar says offline again asks again.
- Tsugiki sends GitHub one request at a time, whichever job asks, so what the user asks for waits behind
  the request in flight, which can take up to 15 s to fail. Once a request has gone 3 s without an answer,
  the status bar shows what it asks and for how long ([status-bar.md](status-bar.md)).
