# gh-rev scope

Terms in **bold** are defined in [CONTEXT.md](../CONTEXT.md). The reasons behind the architecture are in [docs/adr](adr/).

## v1

### Platform and stack

- gh-rev replaces the email inbox for the **Repositories** the user adds: their GitHub work is taken in there, and a single Issue or PR is opened in the browser only now and then. Repositories that aren't added stay outside gh-rev.
- Any number of Repositories, worked in one at a time: `python/cpython` and the user's own projects. Each has its own Workbench, Filters and Pinned issues. Per-repository conventions (CPython's `gh-NNNNN:` title prefix, `[3.x] … (GH-NNNNN)` Backports and `miss-islington`; elsewhere GitHub's "Fixes #N" links) are kept in one place.
- A Tauri desktop app with a Svelte 5 + TypeScript frontend, for Windows and macOS ([ADR 0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)).
- Diffs are drawn by a custom virtualized renderer, with syntax highlighting done in Rust.
- Code comes from gh-rev's own bare mirror. PRs, reviews, comments and CI come from the GitHub API ([ADR 0002](adr/0002-gh-rev-owns-its-git-state.md)).
- Git is driven through the `git` CLI ([ADR 0003](adr/0003-git-cli-over-libgit2-and-gitoxide.md)).
- The GitHub token comes from `gh auth token`, with a personal access token in settings as the fallback ([ADR 0004](adr/0004-auth-via-gh-cli-token.md)).

### Layout

- A **Home page** lists the Repositories, each with what waits on the user there (Review requests, Stale reviews, failed CI on Own PRs, Dependency updates ready to merge) and when it was last opened, plus **Add repository**. With no Repositories yet it shows onboarding instead: `gh auth`, the first Repository, an existing clone to borrow objects from. A normal launch reopens the last Repository; the Home page is one step away from the Repository switcher.
- Inside a Repository the app uses three panes, like a mail client. This layout was chosen out of three that were prototyped on branch `prototype/app-shell`.
  - **Sidebar** (left): the Repository switcher on top, then the Workbench, the Built-in views, All issues, the Filters and the Pinned issues. `Ctrl+B` shows or hides it.
  - **Issue list** (middle): one item per Issue for whatever is selected in the Sidebar, laid out like mail. **Standalone PRs** and **Dependency updates** are items of their own beside the Issues. Each item shows:
    - the people involved
    - the Issue's number and title, with the number of PRs and the time of the last update
    - a preview of what is waiting on the user

    An item that Needs attention is marked the way unread mail is. An Issue's PRs aren't listed here. Any item can be **Archived** or **Muted**, and its author **Muted**.
  - **Detail pane** (right): the selected item. That is either an **Issue view** or a **PR view**.
    - The Issue view shows the title and its author, the Origin PRs, the description, and recent comments, with a box to comment. Each Origin PR's Backports are folded into one line that gives their branches and states.
    - The PR view shows Conversation and Files as tabs.

    Esc goes from a PR back to its Issue.

### Workbench

- The **Workbench** lists every **Issue** that a **Filter** reaches, every **Pinned issue**, and every Issue with an open PR the user holds a **Review** on. An Issue being closed on GitHub does not remove it.
- **Filters** are written in raw GitHub search syntax, with `repo:` of the current Repository added implicitly. They run with GitHub's advanced search, which is the only mode that understands `OR` and parentheses.
- Clicking a person's name opens a temporary `author:<login>` Filter. It lists their Issues and the Issues their PRs fix. It stays in the Sidebar, marked unsaved, until the user saves it as a Filter or closes it.
- **All issues** is an unsaved Filter over the Repository's open Issues, newest update first, with a search box that takes GitHub search syntax; Save turns a search into a Filter. From it the user reads, comments on and pins Issues. It shows what changed since the last sync first and loads older Issues only as the user scrolls or searches.
- The Sidebar shows counts the way a mail client shows unread counts. For the Workbench, the Filters and the Pinned issues, the count is how many Issues Need attention. For the Built-in views, it is how many PRs they hold.
- **Built-in views**: Stale reviews, New activity, Review requests, and Hidden (Archived and Muted items and Muted authors, each with a way to bring it back).
- Each Issue lists its **Matched PRs** first, followed by its other PRs that are open or that **Need attention**.
- **Needs attention** means work waiting on the user:
  - on a PR: a Review request, a Stale review, failed CI on an Own PR, or New activity on a PR the user reviewed or wrote. Unreviewed PRs by colleagues are listed but don't count.
  - on an Issue or a PR's conversation: someone else posted since Last seen, and the user opened it, is assigned, commented in it, or is mentioned in the new posts. An Issue the user only follows shows New activity without needing attention.
  - in a **Maintained repository** (one the user administers on GitHub): every open PR someone else opened, until the user merges, closes or reviews it. Dependabot's PRs fold into one **Dependency updates** item, which needs attention only when one of them is ready to merge or its CI failed.
- An Issue with nothing that Needs attention moves to a "quiet" section at the bottom, unless it is pinned.
- A PR that fixes no Issue is a **Standalone PR**: its own item in the Issue list.
- A Pinned issue stays until the user unpins it.
- Hiding, all kept in gh-rev only:
  - **Archive** puts an item away until something new needs attention there.
  - **Mute** hides an item until the user unmutes it.
  - **Mute author** hides the Issues and Standalone PRs a person or bot opened. Their PRs inside a followed Issue stay listed without needing attention, and an Issue they opened still shows when another PR in it needs attention or a Filter matches it.
  - A Review request or an assignment to the user always shows, even when Muted.

### Issue tree

- Each **Origin PR** shows its **Backports** beneath it, and every PR shows its state, CI status, and the user's review state.
- All of an Issue's fully merged Origin PRs and their Backports collapse into a single summary line.
- Only Backports that already exist are shown; gh-rev does not try to work out which ones are missing.
- **Draft PRs** get a badge.
- A PR that fixes several Issues shows under each of them, with "also fixes #N" on its row.
- **Stacked PRs**: a PR that targets another PR's branch is stacked on it. Its row and PR view say "stacked on #N", and the PR view lists the whole stack in order. gh-rev reads "Closes #N" from a Stacked PR's description itself, because GitHub ignores it there. When the PR below has merged and a Stacked PR still targets its branch, gh-rev warns that the Stacked PR will not reach the default branch.

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
- The first sync of a Repository depends on its size. A small one (a few hundred open Issues and PRs at most) loads everything open. A large one like CPython loads the last 30 days of updates plus everything the Filters, pins and Reviews reach, and older Issues on demand. The threshold and the window are settings.
- Filters, Pinned issues and keybindings are kept in a portable config file that the user syncs themselves. Local marks and Last seen times are kept in a local database.
- Offline, gh-rev is read-only: it shows the mirror and the cached GitHub data. Writing anything needs the network.

## Not in gh-rev

- Merging, editing labels, and requesting reviewers. These stay on GitHub.

## Later

- An interdiff showing only what changed since the user's last Review, built on `git range-diff`. It will be prototyped before it is built.
- LSP-based navigation (clangd, pyright) as a second implementation of symbol lookup.
- Drafting comments offline, with an outbox that pushes them into the Pending review on reconnect.
- Export and import of the local database, so marks and Last seen can follow the user between machines.
- Linux support, which needs the diff renderer's performance checked on WebKitGTK first.

## Ruled out

- Detecting missing Backports. Version labels on Issues don't reliably show which Backports are wanted.
- Importing gh-dash configuration.
- Using GitHub notifications as the baseline for New activity. That baseline is Last seen.
- Using GitHub notifications to decide what is waiting on the user, or covering repositories the user hasn't added. gh-rev works it out from GitHub's data for the added Repositories only.
- A combined Workbench across Repositories. Conventions differ, Issue numbers clash, and CPython's volume would bury the rest.
