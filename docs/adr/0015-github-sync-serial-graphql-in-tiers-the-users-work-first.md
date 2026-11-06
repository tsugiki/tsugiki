# GitHub sync runs serial GraphQL in tiers, the user's work first

Tsugiki reads GitHub with GraphQL, one request at a time, in pages of 50 (25 to 30 when CI rollups are
included). After a 502 or a timeout it retries once at half the page size. Every response's `rateLimit` is
read, and below a floor Tsugiki pauses until the reset, because the 5,000 points an hour belong to the user
and other tools share them. REST is used only for conditional (`ETag`) change checks on small Repositories,
which cost nothing when nothing changed. Searches opt into advanced search (`type: ISSUE_ADVANCED`); the
default mode returns nothing for `OR` with parentheses. The numbers come from [How can GitHub data sync
incrementally within rate limits?](https://github.com/tsugiki/tsugiki/issues/9) (branch
`research/github-sync`). Points don't limit Tsugiki: CPython's first sync costs about 570 and steady state
about 140 an hour. GitHub's 10-second query timeout does.

## First sync

When a Repository is added, one query counts its open Issues and open PRs. Up to 500 is a small Repository:
everything open is loaded, plus whatever closed in the last 30 days. Above that, a large one is loaded through
a window of the last 30 days. The add dialog shows the count, the choice, and the expected time and API cost,
and both numbers are settings on the Repository's page. The first sync counts again with the same query when
its queue starts, and a resumed sync keeps that count, so a Repository that crossed 500 after it was added
gets the sync its size calls for then.

The order puts what waits on the user first, so the Workbench is right long before the sync ends (about 20
minutes for CPython):

1. The user's own items: Review requests, open PRs with the user's **Review**, **Own PRs**, assignments, then
   each **Filter**. Each is a search for IDs, then the missing items in batches of 50. Tsugiki brackets a
   Filter's search, as `repo:<repository> (<filter>)`, so an `OR` in it stays inside the Repository. For a
   Filter with over 1,000 results, the search cap, Tsugiki runs the same search over `created:` date ranges
   and halves each range still over the cap. One day with over 1,000 results of its own loses the rest, and
   the Filter says so in the Sidebar. A search GitHub refuses keeps what it last found, says why in the
   Sidebar and stops nothing else. The Workbench's views and the Home page's counts are right after about a
   minute.
2. Whole trees for the Issues the user's work reaches ([ADR
   0011](0011-pr-links-from-github-signals-by-repository-conventions.md)).
3. Everything else in the window, as one queue. A page of Issues, newest first, and a batch of PRs take turns,
   so rows arrive from the start and a PR in the window fills in soon after its Issue. Each Issue on a page
   brings its latest 50 cross-references, and the PRs they list that changed inside the window join the queue.
   Once the Issues end, pages of the PR list add the PRs no Issue listed. Those pages ask for the ID, number
   and update time of 100 PRs, so each takes little of GitHub's time. All issues fills in as pages land.
   Issues that the fetched PRs name but no page brought come next, then Origin PRs, by number.
4. For a small Repository, its open items that last changed before the window, in the same queue once the
   window's lists end: open Issues, then open PRs. GitHub can't list items by a time they changed before, so
   each list runs oldest first and ends at the first item the window holds. Closing an item changes it, so the
   window already holds what closed inside it: none of 400 closed CPython Issues and PRs had closed after
   their last change. The status bar calls this part "Syncing older open items". GitHub can't count those
   Issues, so the count of Issues has no total until the last list of them ends.

No PR is fetched twice, and a frozen PR is fetched again only if the list shows it changed. A PR older than
the window waits until the user needs its Issue's tree. If the user opens an Issue while the queue runs, that
Issue goes first: all its cross-references, then its PRs, which leave the queue, then their Origin PRs.
Opening one later reads the same whole tree, once per Issue until the app restarts or another Repository
opens, ahead of any sync. That read doesn't count as a sync, and a failure that only concerns this Issue goes
to the log, so the next open tries again.

Requests stay serial because GitHub allows 60 seconds of GraphQL CPU time a minute, which it estimates from
total response time, and a batch of 25 PRs takes about 6 seconds. A second thread would only reach that limit
sooner.

Measured on python/typeshed ([#120](https://github.com/tsugiki/tsugiki/issues/120)), a small Repository with
385 open items, the oldest unchanged since 2018: the add dialog counted 577 items and estimated 150 seconds
and 67 points. The sync took 150 seconds and 44 queries, so about 44 points, and loaded 592 items. 6 of those
queries walked the open items from before the window.

Measured on CPython ([#132](https://github.com/tsugiki/tsugiki/issues/132)), for an account with 108 PRs in
its own trees: the user's own rows arrived after 20 seconds, the first rows of the window after 72, and the
Issues' rows ended after about 5 minutes. The whole sync took 20.5 minutes and 457 queries, and 13.5 of those
minutes went on 144 batches of PRs. It holds 3,631 PRs and 2,368 Issues. Opening an Issue afterwards took 1.7
and 4.2 seconds on two Issues with 14 and 19 cross-references.

The mirror is cloned at the same time ([How is a Repository's mirror created and kept
current?](https://github.com/tsugiki/tsugiki/issues/13)). Until it is done, a PR's conversation shows and its
diff says how far the clone has got. The status bar shows the phase and, in the queue, the Issues read of
their total and the PRs fetched of those known so far. That second count grows as the lists bring more PRs.

Each page of a first sync is committed with its place ([ADR
0010](0010-one-sqlite-database-and-per-repository-mirrors.md)). The place holds the cursors and the queue, so
quitting, a crash, going offline or leaving the queue after 10 failed tries
resumes at the next page. The place is saved after the page, in a transaction of its own, so a crash between
the two reads that page again.

Syncs of several Repositories share one queue and take turns, one step each, so a big first sync can't keep
the others waiting. The open Repository has every other step, and the others take theirs in the order the
user asked for them, the latest first. The open Repository's timed refresh used to wait behind a first sync
of CPython, and a new PR took 12 minutes to show
([#161](https://github.com/tsugiki/tsugiki/issues/161)). While other Repositories sync, the status bar names
them beside the open one's sync time. A failure that speaks for all of GitHub, such as going offline or a
spent rate limit, holds back every step. Any other failure holds back that Repository alone until the next
status refresh, and counts toward its 10 tries. The status bar then names that Repository, unless it's the
open one.

A refresh keeps no place. One that stopped leaves a mark, and the next refresh looks back from the same time
and reads the trees it still owed.

## Afterwards

- **Workbench refresh**, every few minutes for the open Repository: one request for Issues updated since the
  last poll, the newest PRs (paging only while all are newer), each Filter narrowed by `updated:>=`, and a
  probe of the tracked open PRs (Own PRs, PRs with the user's Review, Review requests) for CI and reviews. CI
  finishing doesn't change a PR's `updatedAt`, so this probe is the only way to see it. The request lists
  changed PRs by ID, and a batch then fetches only those Tsugiki holds an older copy of, so a quiet refresh
  costs a few points. Small Repositories first ask REST with `If-None-Match` and skip that request on a `304`.
  The probe still runs, because a `304` says nothing about CI.
  After it, one more request names the failed checks of each Own PR whose CI failed since Tsugiki last
  asked. A CPython PR runs about 70 checks, too many to read with every fetch.
- **A later launch** runs the same refresh from the stored last poll time. A Filter with no stored results,
  such as one just edited in `settings.toml`, still gets its whole search. Trees are read only for Issues the
  refresh found changed or newly reached.
- **Each poll looks back two minutes further than the last one**, because GitHub's search can lag behind what
  it changed. A Filter member whose change is at least two minutes old, and that the narrowed search didn't
  return, has stopped matching. A newer change waits for the next refresh, whose window still holds it.
- **A refresh's times come from GitHub's clock**, because GitHub's `updatedAt` is what they meet. Each
  answer GitHub sends, a `200` or a `304`, carries a `Date` header, and Tsugiki keeps how far the last one is
  from the local clock. GitHub's time is the local time plus that skew, with no request of its own, and it
  stays right after the computer sleeps. Another status can come from a proxy, so its `Date` doesn't count. A
  sync's start, which sets the next poll's `since`, the two-minute limit that keeps a change in a Filter, the
  wait for a rate limit's reset and the `created:` dates of a Filter search all go by it. The session asks who
  is signed in before a sync steps, so the local clock stands in only when that failed, as it does offline at
  launch. The time of the last sync's end stays on the local clock, because the status bar counts "Synced 2
  min ago" from the window's clock. A skew over a minute goes to the log, and again when it moves by over a
  minute. A clock outside the dates of GitHub's certificate fails every request before GitHub answers, so no
  `Date` can help. The status bar then asks the user to check the date and time, and a sync tries again at
  each status refresh, as it does offline, with no count toward giving up.
- **An open Issue or PR** is fetched when opened and about once a minute while on screen. **Merged and closed
  PRs** are fetched once and frozen for what the link rules read; their open Backports stay live. A refresh
  still fetches one that moves, because its timeline goes on: the bot's comment that a Bot backport failed can
  land after a sync has saved the merge.
- **The PRs the user's work reaches** (Review requests, Own PRs, PRs with the user's Review, each Filter's PRs
  and the PRs in their Issues' trees) are kept ready to open
  ([#157](https://github.com/tsugiki/tsugiki/issues/157)). Each fetch of one reads its `headRefOid`. A step
  that saves a head the **Mirror** lacks hands it to the mirror's worker, which fetches the heads it has been
  handed in one `git fetch --stdin` and takes the ones handed meanwhile in the next. git spends no points, so
  only Offline holds it back, as it holds a sync. When a refresh finds one of these PRs changed, a step after
  it reads the PR's conversation, Reviews and threads into the database, so opening it shows them at once. Any
  other PR's head is fetched, and its conversation read, when the user opens it, ahead of what waits.
- **Older items on demand.** Opening one, by a link, a `tsg` URL or "also fixes #N", fetches it and its tree,
  and it stays cached. A search typed in All issues also runs on GitHub, and results the cache lacks are
  added. All issues ends with "Load 30 more days", which walks further back once without changing the window.
- **Window changes.** Widening it walks only the added days. Narrowing it deletes nothing. Items that leave
  the window stay cached and come back through the refresh when updated; nothing is pruned.

## Considered Options

- **REST for the lists.** Reviews and CI would need at least three requests per PR, about 9,600 for CPython's
  30 days, nearly two hours of budget against GraphQL's 130 points.
- **Walking the Repository's Issue and PR lists** for a Filter over the search cap. Lists can't apply search
  syntax, so Tsugiki would need its own copy of GitHub's search language to tell which items match.
- **The open Repository's sync first, to its end.** With CPython open for its first sync, every other
  Repository would wait about 15 minutes, and a Repository whose sync keeps failing would hold the rest back
  until it gave up.
- **Parallel requests**, such as one thread for Issues and one for PRs. Faster on paper, but GraphQL's
  CPU-time limit and the user's shared budget make serial the safe choice, and the first phase is quick
  anyway.
- **The window, then the trees.** The user's own Issues waited 15 minutes for their trees this way, in a sync
  of 15 minutes that sent 78 cross-reference queries, one Issue each. Listing PRs whole took 5.4 seconds a
  query against under 1 for the light list, and the batches fetch those PRs whole anyway.
- **A whole tree for each Issue in the window** ([#129](https://github.com/tsugiki/tsugiki/issues/129)). It
  took 27 minutes and 542 queries and held 5,091 PRs. Its 57 extra batches of PRs took about 5.5 of those 6.5
  extra minutes.
- **The window first, then Filters and Reviews**, which is simpler but leaves an old Review request missing
  for minutes. **Showing nothing until the sync ends.**
- **Other defaults:** 1,000 open and 14 days, or 300 and 60 days. CPython's first sync would take about half
  or twice as long.
- **Only fetching older items when opened**, with searches local to the window. **A background backfill** of
  every open Issue, which costs a few hundred points over hours for rows the user may never look at.
- **Restarting an interrupted first sync**, and **pruning** items outside the window to keep the database
  small.
