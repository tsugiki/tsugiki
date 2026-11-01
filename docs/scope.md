# gh-rev scope

Terms in **bold** are defined in [CONTEXT.md](../CONTEXT.md). The reasons behind the architecture are in [docs/adr](adr/).

## v1

### Platform and stack

- `python/cpython` only. CPython's conventions (the `gh-NNNNN:` title prefix, the `[3.x] … (GH-NNNNN)` backport titles, `miss-islington`, `main` plus maintenance branches) are kept in one place so other repositories can be added later.
- A Tauri desktop app with a Svelte 5 + TypeScript frontend, for Windows and macOS ([ADR 0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)).
- Diffs are drawn by a custom virtualized renderer, with syntax highlighting done in Rust.
- Code comes from gh-rev's own bare mirror. PRs, reviews, comments and CI come from the GitHub API ([ADR 0002](adr/0002-gh-rev-owns-its-git-state.md)).
- Git is driven through the `git` CLI ([ADR 0003](adr/0003-git-cli-over-libgit2-and-gitoxide.md)).
- The GitHub token comes from `gh auth token`, with a personal access token in settings as the fallback ([ADR 0004](adr/0004-auth-via-gh-cli-token.md)).

### Layout

- The app uses three panes, like a mail client. This layout was chosen out of three that were prototyped on branch `prototype/app-shell`.
  - **Sidebar** (left): the Workbench, the Built-in views, the Filters and the Pinned issues. `Ctrl+B` shows or hides it.
  - **Issue list** (middle): the Issue trees for whatever is selected in the Sidebar.
  - **Detail pane** (right): the selected item. That is either an **Issue view** (the Issue's description, recent comments and whole PR tree) or a **PR view** (Conversation and Files as tabs).

### Workbench

- The **Workbench** lists every **Issue** that a **Filter** reaches, every **Pinned issue**, and every Issue with an open PR the user holds a **Review** on. An Issue being closed on GitHub does not remove it.
- **Filters** are written in raw GitHub search syntax, with `repo:python/cpython` added implicitly. They run with GitHub's advanced search, which is the only mode that understands `OR` and parentheses.
- **Built-in views**: Stale reviews, New activity, and Review requests.
- Each Issue lists its **Matched PRs** first, followed by its other PRs that are open or that **Need attention**.
- **Needs attention** means work waiting on the user: a Review request, a Stale review, failed CI on an Own PR, or New activity on a PR the user reviewed or wrote. Unreviewed PRs by colleagues are listed but don't count.
- An Issue with nothing that Needs attention moves to a "quiet" section at the bottom, unless it is pinned.
- PRs without a `gh-NNNNN` prefix are grouped under **No issue**.
- A Pinned issue stays until the user unpins it.

### Issue tree

- Each **Origin PR** shows its **Backports** beneath it, and every PR shows its state, CI status, and the user's review state.
- All of an Issue's fully merged Origin PRs and their Backports collapse into a single summary line.
- Only Backports that already exist are shown; gh-rev does not try to work out which ones are missing.
- **Draft PRs** get a badge.

### Review state and activity

- A PR's review state is **Unreviewed**, **Reviewed**, or **Stale review**.
- A **Review** is either a review submitted on GitHub (line comments included) or a **Local mark**.
- A Review becomes stale only when the PR's diff outside **Generated files** changes. A merge from `main`, or a rebase that leaves the diff the same, does not count. Rebases are detected by comparing patch-ids.
- A **Bot backport** shares its Origin PR's review state. A **Manual backport** is reviewed on its own.
- A **Review request** always counts as Needs attention, including on a Bot backport or a Draft PR. A Local mark does not clear it.
- A colleague's Draft PR only Needs attention when it has a Review request. When it becomes ready for review, that counts as **New activity**.
- A Bot backport's Stale review is counted once, on its Origin PR.
- **Last seen** is recorded per PR and per Issue.
- **New activity** covers people's commits, comments and reviews, a failed Bot backport, and CI changes on an **Own PR** or on a PR the user holds a Review on. Other bot activity and label changes do not count.
- An **Alert** fires when CI finishes on an Own PR. Each kind of Alert can be switched off.

### In-app review

- The whole PR scrolls as one list, in unified or side-by-side view.
- Generated files are collapsed by default and shown with line counts. Whether a file is generated comes from `.gitattributes` on the PR's **Target branch**.
- Every **NEWS entry** in the PR is rendered at the top. A PR can have several, or none.
- Comments can cover a single line or a range of lines. They go into the user's GitHub **Pending review**, which is then submitted as Approve, Request changes, or Comment.
- The user can reply to threads, resolve and unresolve them, post `suggestion` blocks, and leave comments on the PR as a whole.
- **Outdated threads** are grouped at the top of their file together with the code they were written against. During a re-review, the user's own unresolved Outdated threads are listed first.
- Symbol peek runs `git grep` against the PR's head commit, or against the base commit for removed lines.
- "Open in editor" runs `code -g` on the **Review checkout**.

### Keyboard

- Every action can be done from the keyboard. Default keybindings follow VS Code, and on macOS `Cmd` replaces `Ctrl`.

| Action | Key |
|---|---|
| Command palette | `Ctrl+Shift+P` |
| Quick open | `Ctrl+P` |
| Go to definition / peek definition | `F12` / `Alt+F12` |
| Next / previous change | `F7` / `Shift+F7` |
| Next / previous comment thread | `F8` / `Shift+F8` |
| Find in diff | `Ctrl+F` |
| Toggle sidebar | `Ctrl+B` |
| Unified ↔ side-by-side | `Ctrl+Alt+U` |
| Send comment | `Ctrl+Enter` |
| Open in editor | `Ctrl+Shift+E` |
| Local mark | `Ctrl+K M` |
| Pin or unpin | `Ctrl+K P` |

- The command palette's quick open finds Issues, PRs, and files in the current PR. `?` lists the keys available in the current view.
- Keybindings can be remapped in the portable config file.

### Data and freshness

- Polling is tiered and every interval is configurable:
  - The Workbench is refreshed every few minutes, fetching only what has changed since the last poll.
  - An open Issue or PR is refreshed when it is opened, and about once a minute while it stays on screen.
  - Merged and closed PRs are fetched once and then frozen.
  - The user can always refresh by hand.
- Filters, Pinned issues and keybindings are kept in a portable config file that the user syncs themselves. Local marks and Last seen times are kept in a local database.
- Offline, gh-rev is read-only: it shows the mirror and the cached GitHub data. Writing anything needs the network.

## Not in gh-rev

- Merging, editing labels, and requesting reviewers. These stay on GitHub.

## Later

- An interdiff showing only what changed since the user's last Review, built on `git range-diff`. It will be prototyped before it is built.
- LSP-based navigation (clangd, pyright) as a second implementation of symbol lookup.
- Drafting comments offline, with an outbox that pushes them into the Pending review on reconnect.
- Export and import of the local database, so marks and Last seen can follow the user between machines.
- Repositories other than CPython.
- Linux support, which needs the diff renderer's performance checked on WebKitGTK first.

## Ruled out

- Detecting missing Backports. Version labels on Issues don't reliably show which Backports are wanted.
- Importing gh-dash configuration.
- Using GitHub notifications as the baseline for New activity. That baseline is Last seen.
