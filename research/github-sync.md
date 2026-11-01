# Syncing GitHub data incrementally within rate limits

Research for [#9](https://github.com/tsugiki/tsugiki/issues/9). Measured on 2026-11-01 against `python/cpython` with the user's `gh` OAuth token (5,000 points/hour), read-only calls only. "Measured" means a call made for this note; "inferred" means reasoned from docs or from those measurements without a direct test.

## Answer

Use GraphQL for everything Tsugiki syncs, and keep REST for conditional (ETag) change checks on small Repositories. Cost does not limit Tsugiki. A first sync of CPython's last 30 days costs about 220 GraphQL points, and steady-state polling about 140 points an hour, both under 5% of the hourly limit. The real limits are the 10-second query timeout, which pages of 100 PRs with reviews and CI already hit, and CI changes that never bump a PR's `updatedAt`. Filters must use advanced search explicitly: `type: ISSUE_ADVANCED` in GraphQL or `advanced_search=true` in REST. The default mode still returns 0 results for a query with `OR` and parentheses. For CPython, Issues and PRs are linked by the `gh-NNNNN` title prefix and the Issue's cross-reference events. GitHub's closing references cover only 23 of 100 recent PRs and none of the Backports.

## Limits that apply

| Limit | Value | Source |
|---|---|---|
| Primary, REST | 5,000 requests/hour per user, shared by every token and app acting as that user | [REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) |
| Primary, GraphQL | 5,000 points/hour per user | [GraphQL limits](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api) |
| GraphQL point cost | number of connection requests ÷ 100, rounded, minimum 1 | same |
| GraphQL page size / nodes | `first`/`last` 1–100; at most 500,000 nodes per call | same |
| Secondary: concurrency | 100 concurrent requests, REST and GraphQL together | both pages above |
| Secondary: points/minute | 900/min REST, 2,000/min GraphQL; a GraphQL query (no mutation) counts 1 | same |
| Secondary: CPU | 90 s CPU per 60 s real time, of which at most 60 s GraphQL | same |
| Secondary: content creation | 80/min, 500/hour (writes only, not sync) | [REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) |
| Timeout | request terminated after 10 s | [GraphQL limits](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api), [REST troubleshooting](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api) |
| REST search | 30 requests/min; 1,000 results per search; 100 per page; query ≤ 256 chars excluding qualifiers; "no more than five AND/OR/NOT" | [REST search](https://docs.github.com/en/rest/search/search) |
| Advanced issue search | parentheses nest up to five levels | [Filtering and searching issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/filtering-and-searching-issues-and-pull-requests) |
| Conditional requests | a `304` "does not count against your primary rate limit" | [REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api) |

On a secondary-limit `403`/`429`, honour `retry-after`, or else wait at least a minute and back off exponentially; GitHub may ban an integration that keeps retrying ([troubleshooting](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api)). GitHub recommends serial requests rather than concurrent ones ([best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)).

## CPython's volume (measured)

| What | Count |
|---|---|
| Issues updated in the last 30 days | 1,502 (`repository.issues(filterBy: {since})` reports 1,503) |
| PRs updated in the last 30 days | 3,213, of which 740 by `miss-islington` |
| Issues and PRs updated in the last 24 h (a Sunday) | 181, of which 123 PRs |
| Open Issues / open PRs | 6,845 / 2,306 |
| The user's starter Filters: `is:open author:@me` / `is:open involves:@me -author:@me` | 8 / 78 |
| Open PRs `reviewed-by:@me` / `review-requested:@me` | 23 / 2 |

The newest 100 PRs by update time covered about 23 hours.

## Measured costs

| Query | Points | Latency | Notes |
|---|---|---|---|
| 100 PRs by `UPDATED_AT`: state, draft, author, base, head, `closingIssuesReferences(5)`, `reviewRequests(10)`, `latestReviews(20)`, last commit's `statusCheckRollup` | 4 | 9–11 s | the third page in a row failed with **502** (timeout) |
| Same, 50 per page: full 30-day walk, 65 pages, 3,250 PRs | 2/page, **130 total** | mean 5.3 s, max 8.7 s; **344 s** serial | no errors |
| 100 Issues by `UPDATED_AT` with `filterBy: {since}`: labels, assignees, last comment, `closedByPullRequestsReferences` | 4 | — | |
| 100 Issues with `timelineItems(first: 50, itemTypes: [CROSS_REFERENCED_EVENT, CONNECTED_EVENT])` | 1 | — | |
| Full detail of the busiest open PR (#130771: 108 reviews, 65 threads with comments, 42 comments, 268 timeline items, 54 CI contexts) | **1** | — | 150 KB; `reviews(first: 100)` needs a second page here |
| Same with `timelineItems(since: …)` | 1 | — | 0 of 268 items returned: `since` works |
| `nodes(ids: [23 PRs])`: state, head, review requests, latest reviews, CI rollup | 1 | 2.9 s | the CI/review probe for tracked PRs |
| One Workbench delta poll: Issues since T (50), newest 25 PRs with reviews/CI, two Filter searches with `updated:>=T` | **3** | — | |
| GraphQL `search` count-only queries, several aliases | 1 | — | did not touch the REST search bucket (30/30 before and after) |

REST conditional requests (measured, `core` counter read from `x-ratelimit-remaining`):

- `GET /repos/python/cpython/issues?state=all&sort=updated&since=…&per_page=100`, `GET /pulls/130771` and `GET /repos/python/cpython/events`: the first `GET` costs 1; repeats with `If-None-Match` (and `If-Modified-Since` where `Last-Modified` is sent) return `304` and left the counter unchanged in 9 of 10 tries. The one decrement was not reproduced on a rerun; another session shares this token, so it was probably their request (inferred).
- The responses carry `cache-control: private, max-age=60` (lists and PRs) and `max-age=300` (events), so a change can take up to that long to show up (inferred).
- Whether a `304` counts toward secondary limits is not documented.

The GraphQL cost formula counts pages requested, not nodes returned. Nested connections cost one request per parent, so a page of N parents with k nested connections costs about (1 + N·k)/100 points. This is why 50 PRs with 4 nested connections cost 2 points.

## Getting only what changed

| Data | Mechanism | Measured? |
|---|---|---|
| Issues | `repository.issues(orderBy: UPDATED_AT DESC, filterBy: {since: T})` | yes |
| PRs | `repository.pullRequests(orderBy: UPDATED_AT DESC)` has no `since` argument; page until `updatedAt < T` (normally one page of 25) | yes (schema introspection, walk) |
| Filters | advanced search `(<filter>) updated:>=T`. Filter members that are in the changed set but are not returned have stopped matching. Wrapping adds one parenthesis level, so a user's Filter may nest at most four. | count queries measured; removal logic inferred |
| Timelines / activity since Last seen | `timelineItems(since: T)` on Issue and PullRequest | yes |
| Review threads | no `since`; refetch (1 point even for a busy PR). REST review comments have no resolved flag (fields listed on #130771), so thread state needs GraphQL | yes |
| CI | **not** reflected in `PullRequest.updatedAt`: 25 of 40 recently updated open PRs had a check suite updated after the PR's `updatedAt`. Poll `statusCheckRollup` for the PRs Tsugiki tracks. | yes |
| REST lists | `since` on `/repos/{o}/{r}/issues` (includes PRs), `/issues/comments`, `/pulls/comments`; `304` is free | docs + measured 304 |
| Events API | ETag-friendly, `X-Poll-Interval: 60`, but "not built to serve real-time use cases", latency "30s to 6h", 300 events max ([events](https://docs.github.com/en/rest/activity/events)). Not suitable. | docs |

Search results may lag behind the data, because search runs off an index (inferred, not measured). Query `updated:>=` with an overlap of a couple of minutes before the last poll.

## Search in the API

- **Advanced search is opt-in in both APIs today (measured).** For `repo:python/cpython is:issue is:open (label:type-crash OR label:type-security)`:
  - GraphQL `search(type: ISSUE)` and REST `/search/issues` without the flag return **0**.
  - GraphQL `type: ISSUE_ADVANCED` and REST `advanced_search=true` return **254**, which is 192 + 62 for the two labels.
  - Introspection lists `ISSUE`, `ISSUE_ADVANCED`, `ISSUE_SEMANTIC` and `ISSUE_HYBRID`, none deprecated.
  - The [March 2025 changelog](https://github.blog/changelog/2025-03-06-github-issues-projects-api-support-for-issues-advanced-search-and-more/) introduced both switches and notes that in advanced mode a space between `repo`/`org`/`user` qualifiers means AND.
  - Tsugiki should always send the advanced switch. If the default ever changes, the explicit switch should keep working (inferred).
- **Operators:** a query with 6 `OR`s (7 labels) worked in advanced mode (7,005 results), although the REST doc says at most five AND/OR/NOT. The doc's limit may be the legacy one (inferred). Nested `(author:@me OR (review-requested:@me -is:draft))` worked.
- **1,000-result cap:** REST `page=11&per_page=100` returned `422 "Only the first 1000 search results are available"` (measured). `issueCount` still reports the full total (3,213). The GraphQL `search` cap was not tested; assume the same.
  - A Filter that reaches more than 1,000 items, such as "Open Issues" (6,845), cannot be listed through search. List it through `repository.issues(states: OPEN)` (69 pages of 100, about 280–350 points at 4–5 points/page, inferred), or split the query by `created:` ranges.
- **Search rate:** GraphQL `search` does not draw on the REST 30/min search bucket (measured). GitHub documents no GraphQL-specific search limit beyond points.

## Linking Issues to PRs in CPython (measured on 100 recent PRs and 100 recent Issues)

- `PullRequest.closingIssuesReferences` is set on 23 of 100 PRs and on 0 of 32 Backports.
- The `gh-NNNNN` title prefix is present on 93 of 100. Where both exist, the title number was among the closing references in 23 of 23.
- The Issue's `CROSS_REFERENCED_EVENT`s (bedevere writes `gh-NNNNN` into each PR body) found 68 of 71 title-linked PRs, Backports included. The 3 misses came from one Issue with 164 events, truncated by `first: 50`. Use `last: N` or paginate.
- So, as scope.md's per-Repository conventions say: link a PR to its Issue by its title. Use the Issue's cross-reference events to find older PRs and Backports of Issues that Filters or pins reach. Elsewhere, use `closingIssuesReferences`. Cross-references also come from PRs that only mention an Issue, so keep only those whose title names it.

## Recommended strategy

All GraphQL calls run serially, read `rateLimit { cost remaining resetAt }`, and back off below a floor, because other tools share the user's budget. Pages are 50 items, or 25–30 where CI rollups are included. After a 502 or a timeout, retry once at half the page size.

| Data kind | API and query | Cost (CPython) |
|---|---|---|
| **First sync: PRs** in the window | GraphQL `repository.pullRequests(orderBy: UPDATED_AT DESC, first: 50)` with state, draft, author, base/head, review requests, latest reviews, CI rollup; stop at the window edge. Backports come along (they are ordinary PRs). | 65 pages, **130 points, ~6 min** (measured) |
| **First sync: Issues** in the window | GraphQL `repository.issues(filterBy: {since}, first: 100)` with labels, assignees, last comment, `timelineItems(last: 20, itemTypes: [CROSS_REFERENCED_EVENT])` | 16 pages × ~5 = **~80 points** (inferred from the per-page cost) |
| **First sync: Filters, Review requests, Reviews, pins** | GraphQL `search(type: ISSUE_ADVANCED)` returning IDs only, then `nodes(ids:)` in batches of 50 for items not already fetched. Filters over 1,000 results go through `repository.issues`/`pullRequests` instead. | starter Filters + `reviewed-by`/`review-requested`: ~4 searches + a few batches, **~10 points** |
| **First sync total** | | **~220 points, ~100 requests, ~8–10 min serial** (4–5% of an hour) |
| **Workbench refresh** (every 3 min) | One GraphQL request: Issues `since` T, newest 25 PRs (page further only if all are newer than T), each Filter as `(<filter>) updated:>=T`, plus `nodes(ids:)` CI/review probe of tracked open PRs (Own PRs, PRs with the user's Review, Review requests) | **3–4 points/poll, ~70/hour** |
| **Small Repositories** | REST `GET /repos/{o}/{r}/issues?state=all&sort=updated&per_page=1` with `If-None-Match`; run the GraphQL delta only on `200` | **0** when nothing changed |
| **Open Issue/PR detail** (~1/min) | GraphQL single PR/Issue: body, reviews, review threads with comments, comments, `timelineItems(since: last fetch)`, CI contexts; paginate connections over 100 | **1 point/min, ~60/hour** |
| **Merged/closed PR** | Same detail query once, stored and frozen. Its open Backports stay on the live tiers. | 1 point each, once |
| **Diffs, file contents** | the git mirror ([ADR 0002](../docs/adr/0002-tsugiki-owns-its-git-state.md)), not the API | 0 |
| **Steady-state total** | | **~140 points/hour** (~3%) |

REST would cost far more for the first sync (inferred). One page of `/issues?since=` returns 100 Issues and PRs, so the list itself takes about 48 requests. But each PR's reviews and CI need separate requests (`/pulls/N/reviews`, `/commits/{sha}/check-runs`, `/commits/{sha}/status`), at least 3 per PR, which is about 9,600 requests for 3,213 PRs. That is nearly two hours of budget, compared with 130 GraphQL points.

### Secondary limits that could bite

1. **The 10 s timeout** (measured): 100 PRs with reviews and CI rollups took 9–11 s and returned a 502. Keep pages at 50 or below, and smaller where CI is included.
2. **GraphQL CPU time** (60 s per minute): a serial first sync at about 5 s per query is probably safe. Running 4 or more pages in parallel could exceed it (inferred; GitHub does not publish CPU per query). Keep the first sync serial.
3. **REST search 30/min:** avoid REST search. GraphQL search does not use that bucket (measured).
4. **The shared primary budget:** the 5,000/hour belongs to the user, not to Tsugiki. While measuring, the counters moved because of other sessions on the same account. This matters most during a first sync.
5. Concurrency (100), points per minute (2,000 GraphQL), and content creation (80/min) are far away for read-only serial sync.

## Method and rate-limit spend

- Before: GraphQL 4,986 remaining, REST core 4,999, search 30/30.
- Spent by this research, summed from the `cost` fields: about 165 GraphQL points (130 of them for the full 30-day walk), about 10 charged REST core requests (every `304` free), and 3 REST search requests.
- After, from `/rate_limit`: GraphQL 4,907 remaining, core 4,951, search 30/30. Other sessions on the same account used the budget at the same time, and GraphQL had two counters (next point), so before minus after does not equal this research's spend.
- The `rateLimit` readings inside GraphQL responses alternated between two counters, with `resetAt` 16:53:18Z and 16:53:22Z (`used` 93 and 227 at the end). `/rate_limit` showed only the first. Treat `remaining` as approximate and `cost` as exact.
- Each query's fields are listed in the cost tables above. All calls were `gh api -X GET` or GraphQL queries; no mutations.
