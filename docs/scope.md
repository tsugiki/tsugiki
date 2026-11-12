# Tsugiki scope

Terms in **bold** are defined in [CONTEXT.md](../CONTEXT.md). The reasons behind the architecture are in
[docs/adr](adr/). Open points to check or decide before building are filed as [GitHub
issues](https://github.com/tsugiki/tsugiki/issues): project tasks, not **Issues** in the glossary sense.

## v1

The order in which v1 is built is in [build-order.md](build-order.md). This file names what v1 does, and each
section's file in [devel/](devel/) says how it behaves. Read the files a ticket touches.

- Tsugiki replaces the email inbox for the **Repositories** the user adds: their GitHub work is taken in
  there, and a single Issue or PR is opened in the browser only now and then. Repositories that aren't added
  stay outside Tsugiki.

### Platform and stack ([platform.md](devel/platform.md))

- A Tauri desktop app for Windows and macOS, with diffs drawn and highlighted by Tsugiki itself.
- Performance budgets on CPython.
- Code from Tsugiki's own mirror through the `git` CLI, and everything else from the GitHub API.
- `tsg`, the one executable, which also opens a link or an Issue in the running app.
- Unsigned releases built from a tag, and a notice when a new version is out.

### Token ([token.md](devel/token.md))

- The GitHub token from gh or a personal access token, held only by the Rust core.
- The Account page, which says what the token lets Tsugiki do and guides the user to a narrower one.
- Read-only for a Repository the token can't write to.

### Repositories ([repositories.md](devel/repositories.md))

- Any number of Repositories, worked in one at a time, each with its own Workbench, Filters, Pinned issues
  and **Conventions**.
- The **Home page**, which lists them and onboards a new user.

### Layout ([layout.md](devel/layout.md))

- Three panes like a mail client: the **Sidebar**, the **Issue list** and the **Detail pane**. The panes can
  be resized, and the first two hidden.
- Esc, showing only the Detail pane, and Reset Layout.
- The status bar.

### Links ([links.md](devel/links.md))

- Where a link opens: an Issue or PR of the current Repository in the Detail pane, with back and forward
  history; one elsewhere on GitHub in a **Peek**; anything else in the browser.

### Settings ([settings.md](devel/settings.md))

- The Settings dialog and its pages.
- Two TOML files, one that travels between machines and one that stays, and a portable mode.
- `[appearance]` in `settings.toml`: `theme` (System, Light or Dark), and `light_theme` and `dark_theme`,
  the **Theme** of each side.
- A `themes` folder beside `settings.toml`, which holds the user's own **Themes**.
- `[diff]` in `settings.toml`: `files`, whether each PR's Files tab opens on all its files or one at a
  time.

### Themes ([themes.md](devel/themes.md))

- A **Theme** for each side, light and dark, that colours the whole window and follows Windows when it
  changes sides, unless the settings keep one side. VS Code's Light Modern and Dark Modern and Gruvbox's
  Light Hard and Dark Hard are built in.
- A theme picker on `Ctrl+K T` that previews each **Theme** on the window and saves the one picked.
- The user's own **Themes** as TOML files that set a few tokens and take the rest from another **Theme**,
  and Reload themes on `Ctrl+K R`.

### Workbench ([workbench.md](devel/workbench.md))

- The **Workbench**, **Filters**, **Pinned issues**, **All issues** and the **Built-in views**, with counts
  in the Sidebar.
- **Needs attention**, **Standalone PRs**, **Dependency updates**, and hiding an item with Archive, Mute and
  Mute author.
- Mark Seen, which moves **Last seen**, and Mark Unseen, which flags an item **Marked unseen**. Opening an
  item marks nothing, so it keeps its place in the list.
- The Issue tree: Origin PRs with their Backports, **Draft PRs** and **Stacked PRs**.

### Review state and activity ([review-state.md](devel/review-state.md))

- Each PR is **Unreviewed**, **Reviewed** or **Stale review**. A **Review** is a review on GitHub or a
  **Local mark**.
- **Last seen** and **New activity**.

### Alerts ([alerts.md](devel/alerts.md))

- An **Alert** for what only the user can unblock.

### In-app review ([review.md](devel/review.md))

- The whole PR as one list, unified or side by side, with hidden lines on demand, any file collapsible to its
  header, Generated files collapsed from the start, and the NEWS entries on top.
- One file at a time, as an option beside the whole list.
- Threads in place, **Outdated threads**, folded resolved and hidden comments, and each Review as one block
  in the Conversation tab.

### Comments ([comments.md](devel/comments.md))

- Comments on a line or a range into the user's **Pending review**, replies, resolving, `suggestion` blocks,
  and comments on the whole PR.

### Go to definition ([definitions.md](devel/definitions.md))

- **Go to definition** and **Definition peek**, which open a file outside the diff read-only.

### Mirror ([mirror.md](devel/mirror.md))

- Each Repository's **Mirror**, which Tsugiki clones and keeps in shape, borrowing objects from the user's
  clone if asked.
- Open in editor on the **Review checkout**.

### Sync ([sync.md](devel/sync.md))

- Tiered polling, a first sync sized to the Repository, and older items on demand.
- One local database for all Repositories, with Export database and Import database.
- **Read-only** offline.

### Keyboard ([keyboard.md](devel/keyboard.md))

- Every action from the keyboard, with default keys, and remapping.
- The command palette, quick open, and the list of the current view's keys.
- `Ctrl+K` chords, such as `Ctrl+K T` for the theme picker and `Ctrl+K R` to reload themes.
- Tsugiki's keys over the webview's browser shortcuts.

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
