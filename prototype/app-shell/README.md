# PROTOTYPE: whole-app layouts (throwaway)

This prototype lives only on the `prototype/app-shell` branch, and none of its code is meant to reach `main`.

**Question.** What should gh-rev v1 look like, and how should it feel to use? That covers the Workbench, Issue trees, the PR view with conversation and diff, review state, and the keyboard. The answer is usually a mix of the variants, so note what you like in each.

**Plan.** There were three structurally different layouts on one route, chosen with `?variant=A|B|C`. A won (see the verdict below), and B and C are no longer in the working tree: commit `671ae10` on this branch still has all three. `?pr=N`, `?issue=N` and `?author=<login>` open a PR, an Issue or the author Filter straight away.

| Variant | Shape |
|---|---|
| **A**: three panes, like a mail client | Left: Built-in views, Filters, Pinned. Middle: the Issue trees for that scope. Right: the selected PR, with Conversation and Files as tabs. ↑ ↓ move through PRs, and Enter switches tab. |
| **B**: tree first, drill in | The Workbench is a full-width outline (Issue › Origin PR › Backports) with columns. Enter opens an Issue page (its whole tree) or a PR page. The PR page is one long scroll: header, NEWS entries, conversation, then the diff. Esc goes back, and a breadcrumb shows where you are. |
| **C**: workspace like VS Code | Activity bar (Workbench, Views, Filters), a sidebar tree (Ctrl+B), editor tabs with one PR or Issue per tab (Ctrl+W, Ctrl+Tab), a bottom panel for Conversation, Pending review and Commits (Ctrl+J), and a status bar. |

Every key from `docs/scope.md` is wired up: `?` lists them, and `Ctrl+P` / `Ctrl+Shift+P` work everywhere. The code is split into layers (see "Layers" below), and the terms come from CONTEXT.md.

**Data.** `src/lib/data/fixtures/python__cpython.json` is a read-only snapshot of real `python/cpython` data, taken with your `gh` token (the other Repositories: see "Multiple Repositories"). It includes:
- the Filters
- your Review requests and the PRs you have reviewed
- every Issue those reach, plus the Pinned issues gh-159739 and gh-142349, each with its full PR tree
- the full conversation and diff for about a dozen PRs (marked "has diff" in the palette and in B)

Nothing is ever written to GitHub. Local marks, pins, Pending review comments and submitted reviews live in memory, and a reload clears them.

## Run

```sh
npm install                      # in prototype/app-shell; once
npm run proto                    # from the repo root or from prototype/app-shell
npm run proto:snapshot           # refresh the snapshot (needs gh auth; borrows git objects from the clone at GHREV_PROTO_REPO)
npm run proto:check              # layer check, then svelte-check
```

`npm run dev` and http://localhost:1420/ also work in a browser.

## Known simplifications

- The patch-id check runs at snapshot time, only for the user's latest Review on open PRs. A Local mark made in the app is always at the head, so it needs no check.
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

## Issue list feedback, round 1 (layout A)

1. **The badges said little and took a lot of room.** They are replaced by one state dot (open, draft, merged, closed) and short words that only appear when there is something to say: `requested`, `stale`, `reviewed`, `new`, `CI failed`, `manual`. Unreviewed is the default and shows nothing.
2. **`via #N` on Bot backports told the user nothing.** It is gone. Bot backports show no review state of their own, and Manual backports are marked `manual`.
3. **Clicking an Issue (such as gh-159739) should show it.** It now opens in the right pane, with its description, recent comments and the whole PR tree. `&issue=N` opens it from the URL.
4. **Nobody could tell what the green and blue dots meant.** They are replaced by words, and a legend for the state dot sits at the bottom of the Issue list.

## Issue list feedback, round 2: like a mail client (layout A)

The user pointed at their Outlook inbox as the reference. The Issue list now behaves like that inbox:
- **Folded like conversations.** Each Issue is one item with a `›` to unfold its PRs. Selecting an Issue, or one of its PRs, unfolds it, and ← → fold and unfold.
- **Three lines per item.** First the people involved. Then `gh-N` and the title, with the PR count and how long ago it was last updated. Then a preview of what is waiting on the user (`#157178 changed since your review`, `review requested on #140836`), or `5 open · 152 merged` when nothing is.
- **Like unread mail.** An Issue that needs attention gets an accent bar on the left, an accent-coloured title, and bold people.
- **A header over the list** with the scope's name and its sort order (by attention, or by last update).
- **Sidebar numbers count what needs attention,** like unread counts: Issues for the Workbench, Filters and Pinned, and PRs for the Built-in views.

## Issue list feedback, round 3 (layout A)

- **Issues no longer unfold in the Issue list.** It holds one item per Issue, like mail, and ↑ ↓ move between Issues.
- **The Issue view lists the Origin PRs.** Each one with Backports gets a single folded line under it (`› 2 backports: ● 3.15 ● 3.14 (manual)`), which unfolds on click. Clicking a PR opens its PR view, and Esc or the `gh-N` link goes back to the Issue view.
- **Live reload:** keep the `npm run proto` window open. Frontend edits appear in it within a second, and Rust edits rebuild and restart it. Edits to the model or the snapshot reload the page, which clears in-memory pins, marks and comments.

## Author link (layout A)

- **The Issue view shows the author under the title** ("JelleZijlstra opened this 13d ago · 0 comments"), as well as in the breadcrumb. The PR view header's author is clickable too.
- **Clicking an author opens a temporary Filter, `author:<login>`, in the Issue list.** It lists that person's Issues and the Issues their PRs fix, grouped like every other scope. In the Sidebar it shows under Filters as *unsaved*, with **Save** (stub) and ✕. It covers what the person authored only; `involves:` would be a separate Filter. In the prototype it can only see the snapshot, where the real app would search GitHub live. `&author=<login>` opens it from the URL.
- **"Linked PRs" in Issue descriptions are folded** into `▸ Linked PRs (N)`. bedevere-bot maintains that list, and on umbrella Issues it runs long (162 entries on gh-159739). It also repeats the Origin PRs section. 83 of the 101 snapshot Issues have one.
- **Hovering `stale` explains it.** It shows when and at which commit the user reviewed, then what came after: new commits versus merges from main, the time of the last one, and the latest five commits. If the reviewed commit is gone, it says so (force-push or rebase).
- **Hovering `CI passed` / `CI failed` / `CI running`** shows when CI finished, or when the last check started, and on which head commit, flagging a merge from main.
- The snapshot gets these from a separate pass over the open PRs, ten at a time: the last 30 commits, with a merge flag, and the check times on the head. Asking for them inside the 100-result searches makes GitHub time out.

## Patch-id check

A Review is stale only if the PR's own diff changed since the reviewed commit. Merges from `main` and plain rebases don't count.

- **How.** `scripts/fetch/git.mjs` keeps a bare mirror, `.proto-mirror` (git-ignored, about 30 MB; PROTOTYPE, wipe it at will). The mirror borrows objects from the CPython clone through `objects/info/alternates`, which is ADR 0002's read-only bootstrap: the clone is never written. It fetches the target branches and `refs/pull/N/head` for each open PR whose latest Review by the user isn't at the head. For each side (reviewed commit, head), it diffs against the merge-base with the target branch, which is the diff GitHub shows, and compares `git patch-id` file by file. Generated files are left out. A reviewed commit that was force-pushed away is fetched by SHA; if GitHub no longer has it, the Review counts as stale.
- **Whitespace counts.** `git patch-id --stable` ignores all whitespace, and in Python a dedent changes meaning. The check therefore uses `--verbatim` and lists the files whose change is whitespace only. On real data this came up at once: #157178 got a "Lint" commit after the user's Review that only strips a trailing space in `Lib/test/test_gc.py`. Under `--stable` the Review would not be stale; under `--verbatim` it is, and the hover text says "1 file, whitespace only".
- **Result on the snapshot.** 17 open PRs have a Review by the user that isn't at the head. All 17 changed, so the count of Stale reviews stays at 17. In #157178 the merge from main dropped out of the comparison, as it should. The check takes about 1 s per PR, plus about 5 s to fetch.
- **Hover text** on `stale` and `reviewed`: the reviewed commit, the commits since (new commits versus merges from main), then the verdict, either "the PR's diff is the same as when you reviewed it" or the files that changed, each marked if the change is whitespace only.

## Layers

The user asked for data fetching and processing to be cleanly separated from the UI, and the UI from everything else. `npm run proto:check` enforces it (`scripts/check-layers.mjs`):

| Layer | Where | Does | May import |
|---|---|---|---|
| Backend | `scripts/` (Node; Rust in the real app) | GitHub GraphQL (`fetch/github.mjs`), git mirror and patch-id (`fetch/git.mjs`), `snapshot.mjs` puts them together | — |
| Data | `src/lib/data/` | The one place that knows where data comes from: today `fixture.json`, in the app Tauri `invoke` | core (types) |
| Core | `src/lib/core/` | CPython conventions, Review state, Needs attention, Issue trees, the Workbench, diff parsing. Plain TypeScript, no Svelte and no DOM (it runs under plain Node) | core |
| State | `src/lib/state/` | The user's own state (Local marks, pins, Last seen, Pending review) in reactive maps, the actions, and core bound to both | core, data, svelte |
| UI | `src/lib/ui/` | Components, wording and hover text (`format.ts`), syntax highlighting, keys, theme | state, core, svelte, highlight.js |
| Route | `src/routes/` | Reads the URL and mounts `App` | ui |

Core takes the user's state as read-only Map and Set interfaces. When State passes in Svelte's reactive maps, everything derived from core updates as the user acts, and core never imports Svelte. Core returns structured results, such as `ReviewInfo` and `waiting: {pr, reason}`; the wording lives in the UI.

## Multiple Repositories

The design session after the patch-id round settled how gh-rev works across Repositories (CONTEXT.md and docs/scope.md on `main` have the outcome). This round tries it on real snapshots of `python/cpython`, `sergey-miryanov/pep848-sim`, `sergey-miryanov/gcmon` and `sergey-miryanov/gc-scope`.

- **One Repository at a time.** The Sidebar's top line is the Repository; it, `Ctrl+R` and "Repository: …" in the command palette switch. ⌂ goes to the **Home page**, which lists the Repositories with what waits in each (items that need you, review requests, stale reviews, failed CI on yours, PRs to land, dependency updates, new replies) and **Add repository** (stub). `?repo=home` or `?repo=owner/name` opens one from the URL; a normal launch would reopen the last one.
- **Issues by each Repository's convention.** CPython keeps its `gh-N:` title rule; elsewhere a PR goes under every Issue its "Fixes #N" closes, with "also fixes #N" on the row. A PR that fixes no Issue is a **Standalone PR**, an item of its own; the No issue group is gone.
- **Stacked PRs.** pep848-sim's #273 → #276 each target the previous PR's branch. The PR view shows the stack, and gh-rev reads their "Closes #N" itself (GitHub ignores it), so #274 sits under #262. On #274 it warns: *Merged into `xml-late-collections-269` after #273 had merged: its commits never reached main.* That is why #277 was needed.
- **Maintained repositories** (ADMIN on GitHub; all three of the user's): open PRs by others wait on the user ("waits on you"), and dependabot's PRs fold into one **Dependency updates** item that needs attention only when one is ready to merge or its CI failed. gc-scope's nine all have failing CI.
- **Conversations.** An Issue or PR needs attention when someone else posted since Last seen and the user opened it, is assigned, took part or is mentioned ("new reply"). The Workbench also holds anything that needs attention even when no Filter reaches it; the snapshot adds an `involves:@me` search so such Issues are there.
- **Hiding**, in memory: **Archive** (E) until something new needs you, **Mute** (M) until unmuted, and **Mute author** per Repository, with a Built-in view each: **Archived** (Unarchive) and **Muted** (Unmute; then the Muted authors and the items they hide, with Unmute author). `?view=archived` or `?view=muted` opens one. A Review request or assignment always shows.
- **All issues** lists open Issues and open PRs, each PR a row of its own ("fixes gh-N", or Standalone PR), told apart by GitHub-style icons coloured by state. Quick filters **Issues** / **PRs** (with counts; at least one stays on, also in the palette) narrow it. A search box (`label:x`, `author:x`, `#123`, words) covers both; "Updated since the last sync" first, Save as Filter (stub). CPython has the latest page of Issues only, with "Load older…" (stub); the small Repositories have everything open. The Issue view has a comment box (not sent). The kind icon is on every list row, not only here.
- **Links never navigate the app's window.** An Issue or PR in the current Repository opens in the Detail pane, `Alt+←` / `Alt+→` go back and forward; one elsewhere on GitHub opens a **Peek** (cards prefetched by the snapshot), with Open in gh-rev for an added Repository; anything else, or `Ctrl`+click, opens the system browser. `Ctrl+Shift+G`, or a click on the title of an Issue, PR or Dependency updates, opens the current item on GitHub (↗ shows on hover). `?peek=owner/name/N` opens a Peek from the URL.

Code: the snapshot writes one fixture per Repository (`src/lib/data/fixtures/`), with a git mirror each under `.proto-mirrors/` (only CPython's borrows from a clone). Conventions live in `core/conventions.ts`, link routing in `core/links.ts`, and opening the browser in a new `platform` layer that the UI may use (`npm run proto:check` knows it). `npm run proto:snapshot owner/name` refreshes one Repository.

## Settings

The question: what should Settings look like? Three variants open inside the app: `Ctrl+,`, ⚙ in the Sidebar or on the Home page, or "Preferences: Open Settings" / "Preferences: Keyboard Shortcuts" / "Preferences: Settings for <Repository>" in the command palette. The floating bar (or `←` / `→`) switches variant; `?settings&variant=A|B|C` opens one from the URL (`?settings=Keybindings`, `?settings=repo:owner/name` lands on a section).

- **A, Searchable list.** The whole window, like VS Code's settings editor: one list of every setting with its description, a contents column, a search box over settings, keys and Repositories, a bar on changed settings with reset. Changes apply at once. "Show files" puts the two files beside it.
- **B, Dialog.** A preferences window over the app with six pages (General, Account, Repositories, Sync, Keyboard, Advanced), labels on the left, radios for choices, Repositories as master–detail. Apply / Cancel; Esc cancels and puts everything back.
- **C, The files.** No form: `config.toml` and `local.toml` edited as text. On the right, what the line under the cursor means, the problems (each points at its line; valid lines still apply), and everything there is to set. Only the token is a field, because it never goes in a file.

What every variant shows is where each setting lives: **synced** (`config.toml`, which the user syncs: settings, remapped keys, Repositories with their Filters, Pinned issues and Muted authors), **this machine** (`local.toml`: the token source, the editor command, paths, the clone each Repository borrows objects from) or the **keychain** (a personal access token). Pinning or muting in the app shows up in `config.toml` at once.

In effect while the prototype runs: Theme, Code font size, Diff view (for the next Repository opened) and Collapse Generated files. The rest is only written to the files. Keybindings can be remapped and conflicts are flagged, but the app keeps its built-in keys. Nothing is written to disk.

Code: `core/settings.ts` (what the settings are, the two files, a small TOML reader for C), `state/settings.svelte.ts`, `ui/settings/` (the variants, plus the shared Field, KeysTable, RepoForm and Files).
