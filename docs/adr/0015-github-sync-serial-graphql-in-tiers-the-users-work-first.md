# GitHub sync runs serial GraphQL in tiers, the user's work first

Tsugiki reads GitHub with GraphQL, one request at a time, in pages of 50 (25 to 30 when CI rollups are
included). After a 502 or a timeout it retries once at half the page size. Every response's `rateLimit` is
read, and below a floor Tsugiki pauses until the reset, because the 5,000 points an hour belong to the user
and other tools share them. REST is used only for conditional (`ETag`) change checks on small Repositories,
which cost nothing when nothing changed. Searches opt into advanced search (`type: ISSUE_ADVANCED`); the
default mode returns nothing for `OR` with parentheses. The numbers come from [How can GitHub data sync
incrementally within rate limits?](https://github.com/tsugiki/tsugiki/issues/9) (branch
`research/github-sync`). Points don't limit Tsugiki: CPython's first sync costs about 220 and steady state
about 140 an hour. GitHub's 10-second query timeout does.

## First sync

When a Repository is added, one query counts its open Issues and open PRs. Up to 500 is a small Repository:
everything open is loaded, plus whatever closed in the last 30 days. Above that, a large one is loaded through
a window of the last 30 days. The add dialog shows the count, the choice, and the expected time and API cost,
and both numbers are settings on the Repository's page.

The order puts what waits on the user first, so the Workbench is right long before the sync ends (about 8 to
10 minutes for CPython):

1. The user's own items: Review requests, open PRs with the user's **Review**, **Own PRs**, assignments, then
   each **Filter**. Each is a search for IDs, then the missing items in batches of 50. Tsugiki brackets a
   Filter's search, as `repo:<repository> (<filter>)`, so an `OR` in it stays inside the Repository. For a
   Filter with over 1,000 results, the search cap, Tsugiki runs the same search over `created:` date ranges
   and halves each range still over the cap. One day with over 1,000 results of its own loses the rest, and
   the Filter says so in the Sidebar. A search GitHub refuses keeps what it last found, says why in the
   Sidebar and stops nothing else. The Workbench's views and the Home page's counts are right after about a
   minute.
2. The window, newest first. All issues fills in as pages land.
3. Whole trees for the Issues the Workbench shows ([ADR
   0011](0011-pr-links-from-github-signals-by-repository-conventions.md)).

The mirror is cloned at the same time ([How is a Repository's mirror created and kept
current?](https://github.com/tsugiki/tsugiki/issues/13)). Until it is done, a PR's conversation shows and its
diff says how far the clone has got. The status bar shows the phase and the pages done.

Each page is committed with its cursor ([ADR 0010](0010-one-sqlite-database-and-per-repository-mirrors.md)),
so quitting, a crash or going offline resumes at the next page. Syncs of several Repositories run one at a
time, the open Repository first.

## Afterwards

- **Workbench refresh**, every few minutes for the open Repository: one request for Issues updated since the
  last poll, the newest PRs (paging only while all are newer), each Filter narrowed by `updated:>=`, and a
  probe of the tracked open PRs (Own PRs, PRs with the user's Review, Review requests) for CI and reviews. CI
  finishing doesn't change a PR's `updatedAt`, so this probe is the only way to see it. The request lists
  changed PRs by ID, and a batch then fetches only those Tsugiki holds an older copy of, so a quiet refresh
  costs a few points. Small Repositories first ask REST with `If-None-Match` and skip that request on a `304`.
  The probe still runs, because a `304` says nothing about CI.
- **A later launch** runs the same refresh from the stored last poll time. A Filter with no stored results,
  such as one just edited in `settings.toml`, still gets its whole search. Trees are read only for Issues the
  refresh found changed or newly reached.
- **Each poll looks back two minutes further than the last one**, because GitHub's search can lag behind what
  it changed. A Filter member whose change is at least two minutes old, and that the narrowed search didn't
  return, has stopped matching. A newer change waits for the next refresh, whose window still holds it.
- **An open Issue or PR** is fetched when opened and about once a minute while on screen. **Merged and closed
  PRs** are fetched once and frozen; their open Backports stay live.
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
- **Parallel requests.** Faster on paper, but GraphQL's CPU-time limit and the user's shared budget make
  serial the safe choice, and the first phase is quick anyway.
- **The window first, then Filters and Reviews**, which is simpler but leaves an old Review request missing
  for minutes. **Showing nothing until the sync ends.**
- **Other defaults:** 1,000 open and 14 days, or 300 and 60 days. CPython's first sync would take about half
  or twice as long.
- **Only fetching older items when opened**, with searches local to the window. **A background backfill** of
  every open Issue, which costs a few hundred points over hours for rows the user may never look at.
- **Restarting an interrupted first sync**, and **pruning** items outside the window to keep the database
  small.
