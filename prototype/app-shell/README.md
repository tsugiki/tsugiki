# PROTOTYPE: whole-app layouts (throwaway)

This prototype lives only on the `prototype/app-shell` branch, and none of its code is meant to reach `main`.

**Question.** What should gh-rev v1 look like, and how should it feel to use? That covers the Workbench, Issue trees, the PR view with conversation and diff, review state, and the keyboard. The answer is usually a mix of the variants, so note what you like in each.

**Plan.** There are three structurally different layouts on one route, chosen with `?variant=A|B|C`. **Alt+← / Alt+→**, or the bar at the bottom, switch between them. `&pr=N` opens a PR straight away, so the same PR can be compared across variants.

| Variant | Shape |
|---|---|
| **A**: three panes, like a mail client | Left: Built-in views, Filters, Pinned. Middle: the Issue trees for that scope. Right: the selected PR, with Conversation and Files as tabs. ↑ ↓ move through PRs, and Enter switches tab. |
| **B**: tree first, drill in | The Workbench is a full-width outline (Issue › Origin PR › Backports) with columns. Enter opens an Issue page (its whole tree) or a PR page. The PR page is one long scroll: header, NEWS entries, conversation, then the diff. Esc goes back, and a breadcrumb shows where you are. |
| **C**: workspace like VS Code | Activity bar (Workbench, Views, Filters), a sidebar tree (Ctrl+B), editor tabs with one PR or Issue per tab (Ctrl+W, Ctrl+Tab), a bottom panel for Conversation, Pending review and Commits (Ctrl+J), and a status bar. |

All three share one in-memory model (`src/lib/proto/model.svelte.ts`) that uses the terms from CONTEXT.md, plus the leaf components (diff, conversation, badges, palette). Only the layout and the navigation differ. Every key from `docs/scope.md` is wired up: `?` lists them, and `Ctrl+P` / `Ctrl+Shift+P` work everywhere.

**Data.** `src/lib/proto/fixture.json` is a read-only snapshot of real `python/cpython` data, taken with your `gh` token. It includes:
- the Filters
- your Review requests and the PRs you have reviewed
- every Issue those reach, plus the Pinned issues gh-159739 and gh-142349, each with its full PR tree
- the full conversation and diff for about a dozen PRs (marked "has diff" in the palette and in B)

Nothing is ever written to GitHub. Local marks, pins, Pending review comments and submitted reviews live in memory, and a reload clears them.

## Run

```sh
npm install                      # in prototype/app-shell; once
npm run proto                    # from the repo root or from prototype/app-shell
npm run proto:snapshot           # refresh the snapshot (needs gh auth; reads .gitattributes from GHREV_PROTO_REPO)
```

`npm run dev` and http://localhost:1420/?variant=B also work in a browser.

## Known simplifications

- A Review counts as stale whenever its commit isn't the PR head. There is no patch-id check, so a merge from `main` also makes it stale.
- Last seen starts at two days before the snapshot for two PRs out of three; the rest have never been seen. Opening a PR updates it.
- Each line of the diff is coloured on its own by highlight.js, and the diff isn't virtualized. The real renderer is the one on `prototype/diff-renderer`.
- Go to definition, peek, Open in editor, Reply and Resolve are stubs that only show a message.

## Findings so far

- **Filters need `ISSUE_ADVANCED` search.** The user's own search, `sort:updated-desc state:open (author:… OR assignee:…)`, returns 0 results with the GraphQL search type `ISSUE` and 11 with `ISSUE_ADVANCED`. Plain queries return the same results in both modes, so the real app should always use `ISSUE_ADVANCED`.
- **"Needs attention" as defined counts almost everything.** Any open PR you haven't reviewed needs attention, including every colleague's PR reached by a Filter. On real data, 57 Issues need attention and very few are quiet. See the open question in the hand-over.

## Verdict, 2026-11-01

- **Layout: A, three panes like a mail client.** The left pane holds Built-in views, Filters and Pinned. The middle pane holds the Issue trees for that scope. The right pane holds the selected PR, with Conversation and Files as tabs. B and C are kept here for reference.
- **Needs attention now means work waiting on the user:**
  - a Review request
  - a Stale review (a Bot backport's is carried by its Origin PR)
  - failed CI on an Own PR
  - New activity on a PR the user reviewed or authored

  Unreviewed colleague PRs are still listed but don't count. On the same snapshot, the count went from 57 Issues needing attention to 16, with 85 quiet. CONTEXT.md and docs/scope.md on `main` carry the new definition.
