# Layout

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

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
    by last update, then the quiet section. The quiet section starts folded behind a muted "Show 4 quiet
    items" line at the end of the list. A click on the line, or "Show Quiet Items" in the command palette,
    unfolds it, and the line turns to "Hide 4 quiet items" above it. ↑ ↓ skip a folded section. Folding it
    moves a cursor in it to the last line above, and opening a quiet item another way, as with `Alt+←`,
    unfolds it. The choice holds for every view while the Repository stays open. The preview says why in
    the words an **Alert** would use: "CI failed: Tests / Ubuntu", "Changes requested by some-reviewer",
    "Review requested from you". An Issue's
    Origin PRs and their Backports are listed under it, and ↑ ↓ stop on each of them. `Enter` or a click opens
    the item or PR under the cursor in the Detail pane. Any item can be **Archived** or **Muted**, and its
    author **Muted**. A footer under the list gives the legend and the list's keys (↑ ↓, E, M, `Alt+←`, `?`
    for all); it can be switched off.

    `Ctrl+Alt+B`, or "Hide Issue List" in the command palette, hides the list with its footer and its edge,
    so the Detail pane gets its room. The same key, or "Show Issue List", brings it back at its width. In a
    text box the key types as usual, since AltGr sends it too. While the list is hidden:
    - ↑ ↓ scroll the Detail pane, and the cursor stays where it was. `Alt+←` and `Alt+→` still go through
      history.
    - E and M act on the Detail pane's item; with a PR open, on the item the PR belongs to. With nothing open
      they do nothing.
    - A click on a view in the Sidebar brings the list back, showing that view.
    - With nothing open, the Detail pane says "Nothing open. Ctrl+Alt+B shows the Issue list.", naming the
      current binding, and a click on the key runs it.

    Hiding the list moves the focus to the Detail pane. Bringing it back moves the cursor and the focus to the
    row of the item the Detail pane shows, or to the cursor's row when that item isn't in the list or nothing
    is open.
  - **Detail pane** (right): the selected item. That is either an **Issue view** or a **PR view**.
    - The Issue view shows the title and its author, the Origin PRs, the description, and recent comments,
      with a box to comment. Each Origin PR's Backports are folded into one line that gives their branches and
      states.
    - The PR view shows Conversation and Files as tabs, and opens on Conversation. `Ctrl+PageDown` and
      `Ctrl+PageUp` switch tabs, and so does `Enter` on the PR's row once the PR is open.

    Esc closes what is on top, wherever the focus is: a dialog, the palette or a Peek first, then the wait
    for a chord's second key, a Definition peek or the diff's search, and last a PR, which goes back to the
    Issue it was opened under. With nothing on top, Esc does nothing. A PR opened from a row of its own (a
    Standalone PR, one of its Backports, a Dependency update) has nothing under it, so Esc leaves it on
    show, as it leaves an Issue. A text box keeps Esc, unless it belongs to what Esc closes, like the search
    box. Esc gives the focus to whatever lay under the thing it closed: going back from a PR moves the cursor
    and the focus to the Issue's row, or to the Detail pane while the Issue list is hidden. Esc leaves the
    panes as they are.

    The edges between the panes can be dragged; a double-click puts the default back. The Issue list has a
    minimum width, and so does the Detail pane, so in a narrow window the Issue list gives way first.

    `Ctrl+K Z`, or "Show Only the Detail Pane" in the command palette, hides the Sidebar and the Issue list
    for reading a long diff or thread, and remembers which of them showed. With neither on show, `Ctrl+K Z` or
    "Show the Panes" brings back the ones it remembers, or both when it remembers none. It is not a mode:
    `Ctrl+B` and `Ctrl+Alt+B` still bring back their own pane, and either of them makes `Ctrl+K Z` forget.
    The status bar stays, and the Issue list's rules for a hidden list apply. Bringing the panes back puts
    the focus where bringing back the list does; with the Sidebar alone, the focus stays in the Detail pane.

    Pane sizes, the Sidebar's state, whether the Issue list shows and what `Ctrl+K Z` remembers belong to the
    machine, like the window's position, and are not synced.

    "Reset Layout" in the command palette puts all of that back to the defaults at once, without asking: both
    pane sizes, the Sidebar full and on show, the Issue list on show, and nothing remembered by `Ctrl+K Z`.
    The status bar, the list's footer, zoom, the diff view, the performance HUD and the window stay as they
    are. It has no key until the user gives it one.
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
