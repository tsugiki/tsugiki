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
and both numbers are settings on the Repository's page.

The order puts what waits on the user first, so the Workbench is right long before the sync ends (about 27
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
   so rows arrive from the start and a tree fills in soon after its Issue. Each Issue on a page brings its
   latest 50 cross-references, and their PRs join the queue. An Issue with more reads the rest before
   anything else. Once the Issues end, pages of the PR list add the PRs no Issue listed. Those pages ask for
   the ID, number and update time of 100 PRs, so each takes little of GitHub's time. All issues fills in as
   pages land. Issues that the fetched PRs name but no page brought come next, then Origin PRs, by number.

No PR is fetched twice, and a frozen PR is fetched again only if the list shows it changed. If the user opens
an Issue while the queue runs, that Issue goes first: its cross-references, then its PRs, which leave the
queue, then their Origin PRs.

Requests stay serial because GitHub allows 60 seconds of GraphQL CPU time a minute, which it estimates from
total response time, and a batch of 25 PRs takes about 6 seconds. A second thread would only reach that limit
sooner.

Measured on CPython ([#129](https://github.com/tsugiki/tsugiki/issues/129)), for an account with 108 PRs in
its own trees: the user's own rows arrived after 20 seconds, the first rows of the window after 69, and the
Issues' rows ended after about 6 minutes. The whole sync took 27 minutes and 542 queries, and 19 of those
minutes went on 201 batches of PRs. It holds 5,091 PRs and 2,461 Issues. The order before this one took 15
minutes and read the trees of 78 Issues, where this one reads a tree for each of the 1,444 in the window.

The mirror is cloned at the same time ([How is a Repository's mirror created and kept
current?](https://github.com/tsugiki/tsugiki/issues/13)). Until it is done, a PR's conversation shows and its
diff says how far the clone has got. The status bar shows the phase and, in the queue, the Issues read of
their total and the PRs fetched of those known so far. That second count grows as the lists bring more PRs.

Each page of a first sync is committed with its place ([ADR
0010](0010-one-sqlite-database-and-per-repository-mirrors.md)). The place holds the cursors and the queue, so
quitting, a crash, going offline or giving way after 10 failed tries
resumes at the next page. The place is saved after the page, in a transaction of its own, so a crash between
the two reads that page again. Syncs of several Repositories run one at a time, the open Repository first.

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
- **An open Issue or PR** is fetched when opened and about once a minute while on screen. **Merged and closed
  PRs** are fetched once and frozen for what the link rules read; their open Backports stay live. A refresh
  still fetches one that moves, because its timeline goes on: the bot's comment that a Bot backport failed can
  land after a sync has saved the merge.
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
- **Parallel requests**, such as one thread for Issues and one for PRs. Faster on paper, but GraphQL's
  CPU-time limit and the user's shared budget make serial the safe choice, and the first phase is quick
  anyway.
- **The window, then the trees.** The user's own Issues waited 15 minutes for their trees this way, and the
  window's other Issues never got theirs. Listing PRs whole took 5.4 seconds a query against under 1 for the
  light list, and the batches fetch those PRs whole anyway.
- **The window first, then Filters and Reviews**, which is simpler but leaves an old Review request missing
  for minutes. **Showing nothing until the sync ends.**
- **Other defaults:** 1,000 open and 14 days, or 300 and 60 days. CPython's first sync would take about half
  or twice as long.
- **Only fetching older items when opened**, with searches local to the window. **A background backfill** of
  every open Issue, which costs a few hundred points over hours for rows the user may never look at.
- **Restarting an interrupted first sync**, and **pruning** items outside the window to keep the database
  small.
