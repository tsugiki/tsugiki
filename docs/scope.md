# Tsugiki scope

Terms in **bold** are defined in [CONTEXT.md](../CONTEXT.md). The reasons behind the architecture are in
[docs/adr](adr/). Open points to check or decide before building are filed as [GitHub
issues](https://github.com/tsugiki/tsugiki/issues): project tasks, not **Issues** in the glossary sense.

## v1

The order in which v1 is built is in [build-order.md](build-order.md).

### Platform and stack

- Tsugiki replaces the email inbox for the **Repositories** the user adds: their GitHub work is taken in
  there, and a single Issue or PR is opened in the browser only now and then. Repositories that aren't added
  stay outside Tsugiki.
- Any number of Repositories, worked in one at a time: `python/cpython` and the user's own projects. Each has
  its own Workbench, Filters and Pinned issues. Per-repository conventions (CPython's `gh-NNNNN:` title
  prefix, `[3.x] … (GH-NNNNN)` Backports and `miss-islington`; elsewhere GitHub's "Fixes #N" links) are kept
  in one place: the Repository's **Conventions**, a preset whose fields can be edited ([ADR
  0011](adr/0011-pr-links-from-github-signals-by-repository-conventions.md)).
- A Tauri desktop app with a Svelte 5 + TypeScript frontend, for Windows and macOS ([ADR
  0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)).
- Diffs are drawn by a custom virtualized renderer, with syntax highlighting done in Rust.
- It stays fast on CPython: a usable Workbench 1 s after start-up, view queries within 50 ms, no frame over 25
  ms while scrolling any list or diff, a PR's conversation in 200 ms and its first diff rows in 1 s, and a
  background refresh that drops no frame. CodSpeed guards every PR against regressions, and the budgets are
  measured in the real window before each release ([ADR
  0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- Code comes from Tsugiki's own bare mirror. PRs, reviews, comments and CI come from the GitHub API ([ADR
  0002](adr/0002-tsugiki-owns-its-git-state.md)).
- Git is driven through the `git` CLI, 2.30 or newer
  ([ADR 0003](adr/0003-git-cli-over-libgit2-and-gitoxide.md)).
- `tsg` is the app's executable, and there is one instance of it ([ADR
  0007](adr/0007-core-library-and-thin-tauri-app.md)). On its own, `tsg` starts Tsugiki or brings it to the
  front. `tsg python/cpython#12345`, an Issue or PR URL, or a `tsugiki://` link (what an **Alert** carries),
  opens that item in the running app, in its Repository if it has been added and in a **Peek** otherwise. The
  Windows installer, which installs for the current user without admin rights, puts `tsg` on the PATH; on
  macOS a command in the palette does it, as VS Code does for `code`.
- The GitHub token comes from `gh auth token` by default. A personal access token is a supported choice:
  fine-grained read-only to try Tsugiki, fine-grained with review rights on one owner's repositories, or
  classic `public_repo` ([ADR 0004](adr/0004-auth-via-gh-cli-token.md)).
- Only the Rust core holds the token and talks to GitHub; the webview never sees it ([ADR
  0005](adr/0005-token-stays-in-the-rust-core.md)).
- Releases are unsigned in v1: a per-user NSIS installer for Windows x64 and a universal `.dmg` for macOS,
  built from a tag, attested, and checked with `gh attestation verify` before they are published. Until Stage
  4, Windows gets only the portable `.zip` and the `.dmg` runs only on Apple silicon. Tsugiki doesn't update
  itself. It checks GitHub Releases at start-up and every 6 hours, the status bar says when a new version is
  out, and a click opens its release page; a Settings switch turns the check off. Versions follow SemVer, 0.x
  until v1, with notes kept in `CHANGELOG.md` ([ADR
  0012](adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

### Layout

- A **Home page** lists the Repositories, each with how many items Need attention there and what they are
  (Review requests, Stale reviews, failed CI on Own PRs, New activity, Dependency updates ready to merge) and
  when it was last opened, plus **Add repository**. With no Repositories yet it shows onboarding instead: the
  token (gh's, or a read-only one made from the Account page's guide), the first Repository, an existing clone
  to borrow objects from. A normal launch reopens the last Repository; the Home page is one step away from the
  Repository switcher.
- Inside a Repository the app uses three panes, like a mail client. This layout was chosen out of three that
  were prototyped on branch `prototype/app-shell`.
  - **Sidebar** (left): the Repository switcher on top (also `Ctrl+R`, and "Repository: …" in the command
    palette), then the Workbench, the Built-in views, All issues, the Filters and the Pinned issues. It can be
    full, icons only, or hidden. Dragged narrow (or with « in its header) it keeps only icons with their
    counts; clicking an icon brings the full Sidebar back and opens that view. `Ctrl+B` hides it and brings it
    back as it was.
  - **Issue list** (middle): one item per Issue for whatever is selected in the Sidebar, laid out like mail.
    **Standalone PRs** and **Dependency updates** are items of their own beside the Issues. Each item shows:
    - the people involved
    - the Issue's number and title, with the number of PRs and the time of the last update
    - a preview of what is waiting on the user

    An item that Needs attention is marked the way unread mail is. In the Workbench, a Filter or Pinned
    issues, items that Need attention come first, newest first by when they started to need it, then the rest
    by last update, then the quiet section. The preview says why in the words an **Alert** would use: "CI
    failed: Tests / Ubuntu", "Changes requested by some-reviewer", "Review requested from you". An Issue's
    Origin PRs and their Backports are listed under it, and ↑ ↓ stop on each of them. `Enter` or a click opens
    the item or PR under the cursor in the Detail pane. Any item can be **Archived** or **Muted**, and its
    author **Muted**. A footer under the list gives the legend and the list's keys (↑ ↓, E, M, `Alt+←`, `?`
    for all); it can be switched off.
  - **Detail pane** (right): the selected item. That is either an **Issue view** or a **PR view**.
    - The Issue view shows the title and its author, the Origin PRs, the description, and recent comments,
      with a box to comment. Each Origin PR's Backports are folded into one line that gives their branches and
      states.
    - The PR view shows Conversation and Files as tabs, and opens on Conversation. `Ctrl+PageDown` and
      `Ctrl+PageUp` switch tabs, and so does `Enter` on the PR's row once the PR is open.

    Esc goes from a PR back to its Issue.

    The edges between the panes can be dragged; a double-click puts the default back. The Issue list has a
    minimum width, and so does the Detail pane, so in a narrow window the Issue list gives way first. Pane
    sizes and the Sidebar's state belong to the machine, like the window's position, and are not synced.
- A **status bar** runs along the bottom of every screen, the Home page included, for what is app-wide and has
  no other place. It stays when the Sidebar is hidden.
  - Left, where the eye spends its time: sync status ("Synced 2 min ago", a click refreshes) with the
    background work beside it ("· python/peps syncing", "· Cloning python/cpython 42%"), **Offline:
    read-only** (which tints the whole bar), and problems that stop Tsugiki reading, such as an expired token
    or a low API rate limit, each opening the Settings page that fixes it.
  - Right: unsent **Pending reviews** in any Repository (a click opens the PR), and the first key of a chord
    while Tsugiki waits for the second.
  - Each part can be switched off from the bar's right-click menu or Settings › General, and the whole bar can
    be hidden ("View: Toggle Status Bar"). Being offline or an expired token still shows: the bar comes back
    with only that problem, since nothing else would tell the user.
- Links in descriptions and comments are sorted by where they point. Tsugiki never navigates its own window to
  a web page.
  - An Issue or PR in the current Repository opens in the Detail pane. `Alt+←` / `Alt+→` go back and forward
    through what was opened. A slim bar over the Detail pane shows ‹ and › (greyed out when there's nothing
    that way, the target in the tooltip) and the title of the item shown. Right-clicking an arrow lists the
    recent items that way; there is no other history manager. The mouse's back and forward buttons and the
    command palette (Go Back / Go Forward) do the same. History records only items opened in the Detail pane:
    switching views in the Sidebar adds nothing, and re-opening the item already shown adds nothing. Each
    Repository has its own history, which starts fresh when you switch to it.
  - An Issue or PR anywhere else on GitHub opens in a **Peek** popup that Tsugiki renders from the API: title,
    state, author, description, the last few comments, with Open in browser and, for an added Repository, Open
    in Tsugiki. GitHub can't be embedded in a frame.
  - Anything else opens in the system browser. `Ctrl`+click sends any link to the browser, and `Ctrl+Shift+G`,
    or a click on the title in the Detail pane, opens the current Issue or PR on GitHub (Dependency updates:
    the bot's open PRs there).
- **Settings** is a dialog over the app, opened with `Ctrl+,`, ⚙ in the Sidebar or on the Home page, or the
  command palette. Its left column lists the pages (General, Account, Repositories, Sync, Keyboard, Advanced),
  with each Repository under Repositories as a page of its own. Repositories itself is an overview with Add
  repository. A Repository's page holds its conventions, first sync, the clone to borrow objects from, Filters
  and Muted authors; Pinned issues are not there, since the list can be long: they are managed from the
  Sidebar's Pinned issues (★ on an item, `Ctrl+K P`). Changes preview live; Apply keeps them, and Cancel or
  Esc puts them back. The dialog moves by its title bar, resizes from any edge or corner, and can be
  maximized; its size and place are kept per machine. Each setting is labelled with where it is stored
  (settings file, this machine, database, or keychain).
  - The **Account** page shows who is signed in, where the token comes from, and what it lets Tsugiki do in
    plain words ("read public repositories", "write reviews and comments", "push to any repository you can
    push to"), with gh's or a classic token's scopes. It says what Tsugiki does with the token (ADR 0005). A
    guide makes a narrower token: read-only, reviews on one owner's repositories, or classic `public_repo`,
    each with steps, what it can and can't do, and a link to GitHub's form already filled in. A pasted token
    is checked before it is stored, and the page shows its kind, source and expiry, never the token itself;
    within 7 days of expiry the status bar warns and opens the guide. Replace and Remove act on the keychain
    entry, and Remove falls back to gh when gh is signed in. If the keychain refuses the token or is locked,
    Tsugiki shows its cached data without syncing, and the status bar says so with Try again; the Account page
    offers gh or a new token. A machine with no token, such as a new one or a portable folder brought there,
    says "No token on this machine". This was chosen out of three variants prototyped on branch
    `prototype/app-shell`: a searchable list like VS Code's, this dialog, and editing the config files as
    text.

### Workbench

- The **Workbench** lists every **Issue** that a **Filter** reaches, every **Pinned issue**, and every Issue
  with an open PR the user holds a **Review** on. An Issue being closed on GitHub does not remove it.
- **Filters** are written in raw GitHub search syntax, with `repo:` of the current Repository added
  implicitly. They run with GitHub's advanced search, which is the only mode that understands `OR` and
  parentheses.
- Adding a Repository creates starter Filters, ordinary ones the user can edit or delete. A Maintained
  repository gets "Open Issues" (`is:issue is:open`) and "Open PRs" (`is:pr is:open`); any other gets "Mine"
  (`is:open author:@me`) and "Involved" (`is:open involves:@me -author:@me`).
- Clicking a person's name opens a temporary `author:<login>` Filter. It lists their Issues and the Issues
  their PRs fix. It stays in the Sidebar, marked unsaved, until the user saves it as a Filter or closes it.
- **All issues** is an unsaved Filter over the Repository's open Issues and open PRs, newest update first,
  with a search box that takes GitHub search syntax; Save turns a search into a Filter. Each PR is a row of
  its own, and every row in every Issue list has an Issue or PR icon coloured by state. Two quick filters,
  **Issues** and **PRs**, each with its count, show one kind or both (at least one stays on; also in the
  command palette). From it the user reads, comments on and pins Issues and opens PRs. It shows what changed
  since the last sync first and loads older items only as the user scrolls or searches.
- The Sidebar shows counts the way a mail client shows unread counts. For the Workbench, the Filters and the
  Pinned issues, the count is how many Issues Need attention. For the Built-in views, it is how many PRs they
  hold.
- **Built-in views**: Stale reviews, New activity, Review requests, Archived (the Archived items, each with
  Unarchive) and Muted (the Muted items with Unmute, then the Muted authors in this Repository and the items
  they hide, with Unmute author).
- Each Issue lists its **Matched PRs** first, followed by its other PRs that are open or that **Need
  attention**.
- **Needs attention** means work waiting on the user:
  - on a PR: a Review request, a Stale review, failed CI on an Own PR, or New activity on a PR the user
    reviewed or wrote. Unreviewed PRs by colleagues are listed but don't count.
  - on an Issue or a PR's conversation: someone else posted since Last seen, and the user opened it, is
    assigned, commented in it, or is mentioned in the new posts. An Issue the user only follows shows New
    activity without needing attention.
  - in a **Maintained repository** (one the user administers on GitHub): every open PR someone else opened,
    until the user merges, closes or reviews it. Dependabot's PRs fold into one **Dependency updates** item,
    which needs attention only when one of them is ready to merge or its CI failed.
- An Issue with nothing that Needs attention moves to a "quiet" section at the bottom, unless it is pinned.
- A PR that fixes no Issue is a **Standalone PR**: its own item in the Issue list.
- A Pinned issue stays until the user unpins it.
- Hiding, all kept in Tsugiki only:
  - **Archive** puts an item away until something new needs attention there.
  - **Mute** hides an item until the user unmutes it.
  - **Mute author** hides the Issues and Standalone PRs a person or bot opened, in the current Repository
    only. Their PRs inside a followed Issue stay listed without needing attention, and an Issue they opened
    still shows when another PR in it needs attention or a Filter matches it.
  - A Review request or an assignment to the user always shows, even when Muted.

### Issue tree

- Each **Origin PR** shows its **Backports** beneath it, and every PR shows its state, CI status, and the
  user's review state.
- All of an Issue's fully merged Origin PRs and their Backports collapse into a single summary line.
- Only Backports that already exist are shown; Tsugiki does not try to work out which ones are missing.
- Trees are whole for the user's own Issues and any Issue they open: its older PRs and Backports, and a
  Backport's Origin PR, are fetched even when they fall outside the synced window. Another Issue's row can
  lack those older PRs until it's opened. Links follow GitHub alone; a wrong one is fixed on GitHub, not in
  Tsugiki ([ADR 0011](adr/0011-pr-links-from-github-signals-by-repository-conventions.md)).
- **Draft PRs** get a badge.
- A PR that fixes several Issues shows under each of them, with "also fixes #N" on its row.
- **Stacked PRs**: a PR that targets another PR's branch is stacked on it. Its row and PR view say "stacked on
  #N", and the PR view lists the whole stack in order. Tsugiki reads "Closes #N" from a Stacked PR's
  description itself, because GitHub ignores it there. When the PR below has merged and a Stacked PR still
  targets its branch, Tsugiki warns that the Stacked PR will not reach the default branch.

### Review state and activity

- A PR's review state is **Unreviewed**, **Reviewed**, or **Stale review**.
- A **Review** is either a review submitted on GitHub (line comments included) or a **Local mark**.
- A Review becomes stale only when the PR's diff outside **Generated files** changes. A merge from `main`, or
  a rebase that leaves the diff the same, does not count. Rebases are detected by comparing patch-ids file by
  file, each side diffed against its merge-base with the Target branch. Every whitespace change counts,
  trailing spaces included (`git patch-id --verbatim`): indentation matters in Python, and a false "stale"
  costs only a look. Files whose change is whitespace only are named as such.
- A **Bot backport** shares its Origin PR's review state. A **Manual backport** is reviewed on its own.
- A **Review request** always counts as Needs attention, including on a Bot backport or a Draft PR. A Local
  mark does not clear it.
- A colleague's Draft PR only Needs attention when it has a Review request. When it becomes ready for review,
  that counts as **New activity**.
- A Bot backport's Stale review is counted once, on its Origin PR.
- **Last seen** is recorded per PR and per Issue.
- **New activity** covers people's commits, comments and reviews, a failed Bot backport, and CI changes on an
  **Own PR** or on a PR the user holds a Review on. Other bot activity and label changes do not count.
- A Bot backport has failed when the bot says it could not make it, which counts on the Origin PR, or when
  its CI fails, which counts on it and on its Origin PR. A CI change means CI reached a result, so a push that
  sets CI back to pending is no news.
- On an item the user has never opened, only what happened since the Repository's first sync counts, so a
  first sync doesn't mark months of history as new.
- An **Alert** is raised for what only the user can unblock ([ADR
  0016](adr/0016-alerts-launch-tsugiki-links-own-toasts-on-windows-the-plugin-on-macos.md)): CI finished on an
  Own PR, a Review request, a Review on an Own PR, and a failed Bot backport of the user's Origin PR. A reply
  or mention addressed to the user is a kind that starts off. Each kind can be switched off in Settings ›
  General. None is raised while a Tsugiki window has focus, or for what a first sync finds; a Muted item
  raises only Review requests; more than 3 at once become one Alert that opens the Home page. A click opens
  the PR in its Repository. There is no list of past Alerts: every kind leaves the item Needing attention, at
  the top of the Workbench.

### In-app review

- The whole PR scrolls as one list, in unified or side-by-side view.
- Hidden lines around each change can be shown, as on GitHub: 20 more above or below a hunk, the rest of a
  short gap, or the whole file. They are read from the file at the PR’s head in the mirror, so expanding costs
  no API call. New line comments stay on lines of the diff, because GitHub's API accepts them only there; the
  comment-anywhere of github.com's Files changed page has no public API
  ([research](https://github.com/tsugiki/tsugiki/blob/research/expanded-line-comments/research/expanded-line-comments.md)).
  Expanded lines show no + in the gutter, and hovering it says why: reply to a thread there, or comment on the
  PR.
- Generated files are collapsed by default and shown with line counts. Whether a file is generated comes from
  `.gitattributes` on the PR's **Target branch**.
- Every **NEWS entry** in the PR is rendered at the top. A PR can have several, or none.
- Comments can cover a single line or a range of lines within one hunk. They go into the user's GitHub
  **Pending review**, which is then submitted as Approve, Request changes, or Comment.
- The user can reply to threads, resolve and unresolve them, post `suggestion` blocks, and leave comments on
  the PR as a whole.
- A thread someone started on github.com on a line outside every hunk is shown in place: the hidden gap around
  it opens by itself to show that line with 3 lines either side, marked as outside the diff, and the thread
  sits under it like any other, placed by its file, line and side. It can be replied to and resolved like any
  thread, and `F8` reaches it in order. If its line no longer exists at the head, it is an **Outdated
  thread**.
- **Outdated threads** are grouped at the top of their file together with the code they were written against.
  During a re-review, the user's own unresolved Outdated threads are listed first.
- **Go to definition** (`F12`) and **Definition peek** (`Alt+F12`) find a symbol's definition with `git grep`
  at the line's commit: the PR's head, or the base for a removed line. They know Python, C in CPython's style
  and the reST directives in `Doc/`, and fall back to a whole-word search. A definition in a file the PR
  changes is shown in the diff; elsewhere, Go to definition opens that file read-only at the same commit in
  the Detail pane, and `Alt+←` returns to the diff ([ADR 0002](adr/0002-tsugiki-owns-its-git-state.md)).
- "Open in editor" runs the editor command (`code -g` by default) on the **Review checkout**, moved to the
  PR's current head. If the user has edited files there, Tsugiki first offers Discard and switch, Copy as
  patch, or Cancel. Settings › Repositories shows the checkout's size, with Remove checkout.
- A Repository's **Mirror** is first cloned as the Repository is added, alongside its first sync, one
  Repository at a time with the open one first. Add repository offers to borrow objects from a clone the user
  has, picked as a folder, and gives the time and disk each choice takes. Tsugiki checks that the folder is a
  clone with a remote for that repository, and doesn't search the disk for one. The path is kept in the
  Repository's settings, so a new clone after a broken one borrows again. A clone that fails is deleted and
  tried again, and one cut off by quitting starts again at the next launch.
- While a Repository's mirror is still being cloned, the diff, the definitions and Open in editor say how far
  it has got. The Conversation tab works meanwhile.
- Without git 2.30 or newer, or without room on the disk for the clone, the diff says what is missing, with a
  link to git's downloads or the space needed, and offers Check again. The rest of Tsugiki works without it.

### Keyboard

- Every action can be done from the keyboard. Default keybindings follow VS Code, and on macOS `Cmd` replaces
  `Ctrl`.

| Action | Key |
|---|---|
| Command palette | `Ctrl+Shift+P` |
| Quick open | `Ctrl+P` |
| Go to definition / Definition peek | `F12` / `Alt+F12` |
| Next / previous change | `F7` / `Shift+F7` |
| Next / previous comment thread | `F8` / `Shift+F8` |
| Find in diff | `Ctrl+F` |
| Hide / show the Sidebar | `Ctrl+B` |
| Unified ↔ side-by-side | `Ctrl+Alt+U` |
| Next / previous tab in the PR view | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Send comment | `Ctrl+Enter` |
| Open in editor | `Ctrl+Shift+E` |
| Local mark | `Ctrl+K M` |
| Pin or unpin | `Ctrl+K P` |
| Back / forward | `Alt+←` / `Alt+→` |
| Open on GitHub | `Ctrl+Shift+G` |
| Switch Repository | `Ctrl+R`, or the command palette |
| Archive / Mute the selected item | `E` / `M` |
| Settings | `Ctrl+,` |
| Zoom in / out / reset | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |
| Show or hide the performance HUD | `Ctrl+Shift+H` |

- The webview's own browser shortcuts (Print, Reload, Find, zoom, DevTools) never fire in a release build;
  Tsugiki's keys win, on any keyboard layout. Zoom is Tsugiki's, kept per machine, and Ctrl+wheel doesn't
  zoom. On macOS the standard menu stays, with Settings… on `Cmd+,`; a remap onto one of its shortcuts is
  refused ([ADR 0013](adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)).
- The command palette's quick open finds Issues, PRs, and files in the current PR. `?` lists the keys
  available in the current view.
- Keybindings can be remapped on the Settings Keyboard page: click a key and press the new one; chords such as
  `Ctrl+K M` work, and two actions on one key are flagged. Only remapped keys are written to the portable
  config file.

### Data and freshness

- Polling is tiered and every interval is configurable:
  - The Workbench is refreshed every few minutes, fetching only what has changed since the last poll.
  - An open Issue or PR is refreshed when it is opened, and about once a minute while it stays on screen.
  - Merged and closed PRs are fetched once and then frozen.
  - The user can always refresh by hand.
- The first sync of a Repository depends on its size ([ADR
  0015](adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)). A small one (up to 500 open
  Issues and PRs) loads everything open, plus what closed in the last 30 days. A large one like CPython loads
  the last 30 days of updates; the threshold and the window are settings. Either way it loads what waits on
  the user first (Review requests, PRs with the user's Review, Own PRs, assignments, then the Filters), so the
  Workbench is right in about a minute, then the trees of the user's Issues, then the rest of the window
  newest first, with the PRs that changed in the window, and for a small one its open items from before the
  window. The status bar shows its progress. An interrupted sync resumes where it stopped. Widening the window
  loads only the added days; narrowing it deletes nothing.
- Older items come on demand: opening one fetches it and keeps it, a search in All issues also runs on GitHub,
  and All issues ends with "Load 30 more days".
- Settings are kept in two TOML files, which Tsugiki reads when it starts and when the Settings dialog opens,
  so a hand edit takes effect at the next of those ([ADR
  0014](adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)). `settings.toml`
  travels: settings, remapped keybindings, and each Repository with its Conventions and Filters; the user
  syncs it by pointing `settings_path` at a dotfiles repository or a synced folder. `machine.toml` stays: the
  token source, the editor command, paths, the clone each Repository borrows objects from, window places and
  zoom. A personal access token goes to the OS keychain. Pinned issues, Muted authors, Muted and Archived
  items, Local marks, Last seen times, and text typed into a composer but not yet sent are kept in the local
  database. Unsent text survives a reload, a crash or a refused write, and is cleared once it is sent ([ADR
  0008](adr/0008-view-models-and-invalidation-over-ipc.md)).
- A portable mode works as VS Code's: with a `data` folder next to `tsg.exe` (or `tsugiki-portable-data` next
  to Tsugiki.app), the files, the database and the mirrors all live there. Releases include a Windows portable
  `.zip`.
- The local database is one file for all Repositories, holding both the cached GitHub data and the user's own
  state ([ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). **Export database**
  (Settings › Advanced, and the command palette) writes a consistent copy of it anywhere, such as a synced
  folder. **Import database** on another machine replaces that machine's database after a confirmation,
  keeping a backup, so Pinned issues, Muted authors, Local marks, Last seen, Archived and Muted items and the
  cache follow the user. The mirrors are cloned again on the new machine.
- Offline, Tsugiki is **Read-only**: it shows the mirror and the cached GitHub data. Writing anything needs
  the network.
- A token that can't write to a Repository makes it Read-only too. The status bar says so on the left and
  opens the Account page. The composer stays usable: it offers "Copy and open on GitHub" instead of sending,
  and Submit review is disabled with the reason. With a fine-grained token Tsugiki finds out on the first
  refused write, keeping the text that was being sent.

## Not in Tsugiki

- Merging, editing labels, and requesting reviewers. These stay on GitHub.

## Later

- An interdiff showing only what changed since the user's last Review, built on `git range-diff`. It will be
  prototyped before it is built.
- LSP-based navigation (clangd, pyright) as a second implementation of symbol lookup.
- Drafting comments offline, with an outbox that pushes them into the Pending review on reconnect.
- Merging from Tsugiki in **Maintained repositories**, including Dependency updates in one go. Until then
  merging, labels and reviewers stay on GitHub.
- Linux support, which needs the diff renderer's performance checked on WebKitGTK first.
- A token per Repository owner, for fine-grained tokens that write to Repositories of several owners.
- Signed releases: Developer ID signing and notarization on macOS, and code signing on Windows.
- Updating in the app with Tauri's updater, instead of a notice that links to the release.
- A search box in Settings that finds a setting, a key or a Repository across all pages. Variant A of the
  Settings prototype (branch `prototype/app-shell`) has one to start from.

## Ruled out

- Detecting missing Backports. Version labels on Issues don't reliably show which Backports are wanted.
- Importing gh-dash configuration.
- Using GitHub notifications as the baseline for New activity. That baseline is Last seen.
- Using GitHub notifications to decide what is waiting on the user, or covering repositories the user hasn't
  added. Tsugiki works it out from GitHub's data for the added Repositories only.
- A combined Workbench across Repositories. Conventions differ, Issue numbers clash, and CPython's volume
  would bury the rest.
