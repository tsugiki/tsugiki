# Layout

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Inside a Repository the app uses three panes, like a mail client. This layout was chosen out of three that
  were prototyped on branch `prototype/app-shell`. The panes fill the window above the status bar, and each
  scrolls by itself with a scrollbar at its own right edge. The wheel scrolls the pane under the pointer and
  stops at that pane's end; the window itself doesn't scroll.
  - **Sidebar** (left): the Repository switcher on top, whose picker (also `Ctrl+R`, and "Repository: …" in
    the command palette) starts with Home and ends with Add repository, then the Workbench, the Inbox, the
    Built-in views, the Filters and the Pinned issues. It can be full, icons only, or hidden. « in its header,
    a drag of its edge below 120 px, or "View: Sidebar as Icons" in the command palette shrinks it to a 48 px
    strip of icons; between 120 px and its least width, 140 px, the drag holds it at 140 px. The strip has the
    Repository's initials on top, then an icon for each entry the full Sidebar lists, with its count as a
    badge and its name in the tooltip, and ⚙ at the bottom. A Filter whose last search failed says why in its
    tooltip, and its icon shows a red ! for its count. A click on an icon brings the full Sidebar back and
    opens that view, and a click on the initials or "View: Full Sidebar" brings it back on the view on show.
    Either way it comes back at the last width Tsugiki saved for the full Sidebar, as a drag or a key let go,
    and not at a width a drag passed through on its way to fold. A drag of the strip's edge past 120 px brings
    it back at the width the drag reaches, and a double-click on the edge at its default width. `Ctrl+B` hides
    it and brings it back as it was, full or icons. The strip is not the PR view's rail
    ([review.md](review.md)).
  - **Issue list** (middle): one item per Issue for whatever is selected in the Sidebar, laid out like mail.
    **Standalone PRs** and **Dependency updates** are items of their own beside the Issues. Each item shows:
    - the people involved
    - the Issue's number and title, with the number of PRs and the time of the last update
    - a preview of what is waiting on the user

    The list, and the rest of the window, show a time as "… ago", with its date and time in a tooltip
    ([#1066](https://github.com/tsugiki/tsugiki/issues/1066)).

    A header over the list names the view on show. In the Workbench, a Built-in view, a Filter or Pinned
    issues, a switch at the header's far end, or "Sort by Attention" and "Sort by Last Update" in the command
    palette, picks one of two orders, and `machine.toml` keeps the choice:
    - **By attention**: items that Need attention come first, newest first by when they started to need it,
      then the rest by last update.
    - **By last update**: each item newest first by its last update.

    The Inbox, Archived and Muted keep their own order and show no switch. The items with nothing waiting
    on the user stay in the one list, with no heading or fold of their own.

    One rule moves the cursor and the Detail pane when the selected item leaves its place, in every list
    ([#1052](https://github.com/tsugiki/tsugiki/issues/1052)). Its terms:
    - The **selected item** is the list item, a row of its own, that owns the cursor's row. A PR in an
      Issue's tree is a row but no list item.
    - A **user action** is anything the user does to the selected item, from a key, the palette, a button or
      a menu, with the list hidden too. A **core change** is anything else that changes the list: a sync,
      another window, a change made on GitHub.
    - A user action **finishes** the item when it settles what the item waits on, or the core's answer to
      it leaves the item out of the view on show. Whether it settles comes from the action and the item as
      the window draws it, at the keypress. Whether it took the item out comes from the core, once it has
      kept the action. Any other user action **keeps** the item.

    | #   | Who acts | What it does to the selected item                                              | Cursor                                                                                                  | Detail pane                                        |
    | --- | -------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
    | 1   | User     | Finishes the item                                                              | Moves to the next row once the core has kept the action, or to the last row when nothing comes after it | Opens the row the cursor moved to, as `Enter` does |
    | 2   | User     | Keeps the item                                                                 | Stays on the item and goes with it if it moves                                                          | Keeps showing the item                             |
    | 3   | User     | Acts on a PR in an Issue's tree, and on the PR alone                           | Stays on the PR's row                                                                                   | Keeps showing the PR                               |
    | 4   | Core     | Moves the item but leaves it in the list, into a Filter's open Muted items too | Stays on the item and goes with it                                                                      | Keeps showing the item                             |
    | 5   | Core     | Takes the item out of the list                                                 | Leaves the list: no row is selected, and the next ↓ goes to the first row                               | Keeps showing the item                             |

    In every row, focus on a row of the list goes with the cursor; in row 5, and in row 1 with no other row
    left, it stays on the list with no row selected. Focus anywhere else, as in the Detail pane, a text box
    or the list's header, stays where it is, and focus in the Detail pane stays there in the item it opens.
    The list scrolls as little as it takes to bring the cursor's row back on the screen, and leaves the rest
    to the browser, also when the user had scrolled that row away. Row 1 opens the row the cursor moved to in
    the Detail pane whatever the pane showed before, and opening a row leaves its Last seen alone; with no
    other row left, the pane shows nothing open, and `Alt+←` goes back to the item the user finished with. A
    user action that fails moves nothing, and the problem shows as it does now. A change of view or order is
    neither: the cursor stays on the item wherever the new list puts it, and where it was when the list
    leaves the item out. Revise the whole table for a later action that fits neither "finishes" nor "keeps",
    or a core change that fits neither core row.

    The window sends each action with the selected item and the list on show, and the core answers once it
    has kept the action, with that list as it now stands, matched by the action's id. The cursor and the
    Detail pane move on that answer, and on nothing else. A failed action sends no answer, so nothing waits.

    Today, row 1 takes Mark Seen on the item's own row while the item waits or has something new, By last
    update too; Archive and Mute; Mute author on an item its author opened, and on a PR's row whose author's
    Mute takes the item out of the list; Unarchive, and Mark Unseen of an Archived item, in the Archived view;
    and the Pin and Unpin moves [workbench.md](workbench.md) names. Archive in a Filter's view or the Review
    requests, which can keep the item drawn Archived, moves on but keeps the cursor on the item when it was
    last. `E` and `M` on a PR in a tree act on the Issue, so the cursor moves past the Issue and its tree, and
    `Q` there is row 3.

    An item that Needs attention is marked the way unread mail is, and keeps that look until nothing on it
    waits on the user or the user marks it seen, which drops the whole look. The preview says why in the
    words an **Alert** would use: "CI failed: Tests / Ubuntu", "Changes requested by some-reviewer", "Review
    requested from you", or "Marked unseen". An Issue's Origin PRs and
    their Backports are listed under it, folded at first like a conversation in a mail client: the Issue's
    row shows `›` and its preview, and ↑ ↓ go from Issue to Issue. → or a click on `›` unfolds the Issue,
    and ↑ ↓ then stop on each of its PRs. ← folds it; on a PR's row ← moves the cursor to its Issue, as in
    VS Code's trees, and a second ← folds it. The list takes ← and → only while it has the focus. Opening a
    PR of a folded Issue some other way, such as with `Alt+←` or from the Issue view, unfolds the Issue.
    Each view keeps which Issues are unfolded while the Repository stays open. `Enter` or a click opens the
    item or PR under the cursor in the Detail pane. `Q` marks the row under the cursor seen and `Shift+Q`
    marks it unseen; with the focus in the Detail pane, they act on its item. `E` archives the item that
    holds the row under the cursor, and the cursor moves on as the rule above says; with the focus in the
    Detail pane or the list hidden, it archives the Issue on show, or the item the PR on show belongs to
    while the list holds it, and with nothing open it does nothing. `Q` on an Archived
    item marks it seen and keeps it Archived. A row the list keeps while Archived, as the Review requests
    view keeps a PR whose Review request stood at the archive, says "Archived" beside its age. In the
    Archived view each item's row has Unarchive after its age, and `E` on any of its rows unarchives it too;
    either way the cursor moves on once the core has kept the action, as the rule above says, while `E` with
    the focus in the Detail pane still archives. "View: Archived" in the command palette opens the view. `M`
    mutes the item `E` would archive, and the cursor moves on as for `E`; on a Muted item it unmutes it, and
    `Q`, `Shift+Q` and `E` do nothing. A row the list keeps while Muted says "Muted" beside its age. An item's
    author can be **Muted** too. A footer under the list gives the legend and the list's keys (↑
    ↓, ← →, E, M, `Alt+←`, `?` for all); it can be switched off.

    `Ctrl+Alt+B`, or "Hide Issue List" in the command palette, hides the list with its footer and its edge,
    so the Detail pane gets its room. The same key, or "Show Issue List", brings it back at its width. In a
    text box the key types as usual, since AltGr sends it too. While the list is hidden:
    - ↑ ↓ scroll the Detail pane, `PgDn` and `PgUp` page it, and `Home` and `End` go to its start and end;
      the cursor stays where it was. `Alt+←` and `Alt+→` still go through history.
    - E and M act on the Detail pane's item; with a PR open, on the item the PR belongs to. With nothing open
      they do nothing. Q and Shift+Q act on the Detail pane's item too, and on a PR view that is the PR.
    - A click on a view in the Sidebar brings the list back, showing that view.
    - With nothing open, the Detail pane says "Nothing open. Ctrl+Alt+B shows the Issue list.", naming the
      current binding, and a click on the key runs it.

    Hiding the list moves the focus to the Detail pane. Bringing it back moves the cursor and the focus to the
    row of the item the Detail pane shows, or to the cursor's row when that item isn't in the list or nothing
    is open.

  - **Detail pane** (right): the selected item. That is either an **Issue view** or a **PR view**.
    - Under the title, a button reads "Mark Seen" while the item has New activity or is Marked unseen, and
      "Mark Unseen" otherwise. The command palette's "Mark Seen" and "Mark Unseen" act on what `Q` and
      `Shift+Q` would where the focus was as it opened: the item on show from the Detail pane or with the
      Issue list hidden, and otherwise the line under the cursor ([keyboard.md](keyboard.md)).
    - Atop the Issue and PR views of a **Read-only** Repository, a line says why, for each reason it holds
      ([token.md](token.md)). "you chose it in this Repository's settings" links to the Repository's page in
      Settings, which switches the choice ([settings.md](settings.md)).
    - The Issue view shows the title, an author line, the Origin PRs, the description, and recent comments
      and events, with a box to comment. Between the comments it shows the recent events, one line each, in
      time order: who did what, and how long ago, with the date and time in the tooltip. A cross-reference
      and a hand link name the PR or Issue they point at, a referenced commit its short SHA and first line,
      a close whether it was completed or not planned, a rename the title it had, a label event the label,
      and an assignment the assignee. Each list says how many earlier comments or events only GitHub holds.
      The author line says who opened the Issue and how long ago, with the date and
      time in its tooltip, how many comments it has, and whom it is assigned to, if anyone. Each PR's line
      in the Origin PRs gives its last update in its tooltip, as does each Backport's, and the line that
      folds the merged PRs gives the latest of theirs, so no PR's time takes room on a line
      ([#1066](https://github.com/tsugiki/tsugiki/issues/1066)). Under it the
      Issue's labels show as plain chips in name order, as GitHub lists them, beside a row of buttons:
      Archive, which archives the Issue on show and reads Unarchive on an Archived Issue, Mute, which mutes
      it, "Mute <author>", which mutes the Issue's author and is off on the user's own Issue, then Mark
      Seen. A Muted Issue shows only Unmute and "Mute <author>", and a Muted author's Issue only
      "Unmute <author>". An Issue with no labels shows no chips. Each
      Origin PR's Backports are folded into one line that gives their branches and states. A row between
      the "Origin PRs" heading and the tree counts the PRs by state, as
      `Open 2 · Draft 0 · Merged 1 · Closed 0 · Bold: Needs attention 1`: each state's icon, its word, then
      its count, muted and without brackets, as on GitHub's tabs, with a muted dot between entries. Each
      count matches its icon, so an open draft counts as Draft and not as Open, and the counts take in the
      Backports and the folded merged PRs. All four states show on every Issue, so Open sits in the same
      place; a state at 0 shows in muted colour. The count of PRs that Need attention comes last,
      after "Bold" in bold. A screen reader reads the row as a list named "PR counts", each entry its word
      and count, "Open, 1" or "Needs attention, 1", and skips the icons, the dots and "Bold". Each PR's line
      starts with its branch and ends with its author, and a PR that Needs attention shows in bold. The
      worklist's line for an Issue still says "2 open of 3 PRs", as it has no room for five counts.
    - The PR view of a PR no Issue's tree holds, a Standalone PR, a Standalone Backport or a Backport in a
      Standalone PR's tree, shows Archive, Mute and "Mute <author>" beside Mark Seen, with the rules the Issue
      view's buttons follow: Archive reads Unarchive on an Archived item, a Muted item shows only Unmute and
      "Mute <author>", a Muted author's item only "Unmute <author>", and "Mute <author>" is off on the user's
      own PR (#1077). A PR in an Issue's tree shows only Mark Seen: its `gh-N` crumb opens the Issue its view
      names, where the Issue's buttons are, and `E`, `M` and the palette still act on the Issue from the PR
      view. No ★ shows, as only Issues take a pin
      ([#488](https://github.com/tsugiki/tsugiki/issues/488)). The PR view shows Conversation and Files as
      tabs, and opens on Conversation. `Ctrl+PageDown` and `Ctrl+PageUp` switch tabs, and so does `Enter` on
      the PR's row once the PR is open. A switch leaves the focus where it was: in the Issue list it stays
      there, and in the Detail pane it moves to the new tab's content, so its keys keep acting on the pane.
    - An Issue or PR the database lacks, which quick open's "Fetch #N from GitHub" asks for
      ([keyboard.md](keyboard.md)), shows `owner/repo #N` at once, with "Fetching owner/repo #N from
      GitHub…" under it. The pane takes the focus as a pick from quick open does. The session fetches the
      item ahead of the sync's queued work, and the database keeps it as it keeps an Origin PR outside the
      window. A PR that the next refresh would find changed may have opened since, and brings back an
      Archived Issue it names, as that refresh would. The pane then opens the item as an Issue view or a PR
      view, whichever GitHub says #N is. The focus moves as a pick moves it only from a row, the Detail
      pane or nowhere, and stays anywhere else the user went meanwhile, such as a text box or the Sidebar.
      A fetch that brings nothing says why, with an Open in browser button: GitHub has no Issue or PR #N
      there, the token can't see it, Tsugiki doesn't hold it and can't ask GitHub now, which the status bar
      explains, or something else went wrong, which `tsugiki.log` says. A fetch asked as the status turned
      offline asks GitHub nothing, and says the third at once. Opening anything else meanwhile
      drops the fetch's answer, though the database still keeps what it brought.
    - **Look**, which the prototype settled, with colours from the Theme's tokens ([themes.md](themes.md)):
      - A muted 12 px line over the title gives the Issue's state, or the Issue a PR was opened under, the
        branch it goes into and when it changed. The title is 18 px, its state icon centred on its first
        line; a title too long for the line wraps beside the icon, its later lines under the title's start.
        The 13 px line under it names the author in the text colour at weight 600, and a PR's state words
        follow.
      - The Issue view keeps to 900 px and the Conversation to 860 px, so lines stay short in a wide
        window. Section headings, "Origin PRs" and "Description", are 13 px with 18 px above.
      - The PR tree is a box with 6 px corners and a line between rows. Each row is 13 px, with the number
        in the code font in the accent colour. A PR that doesn't Need attention is muted, and the Backports'
        line sits indented on the panel colour.
      - The description, each comment and each Review with a body sit in a box with 6 px corners: a 13 px
        header on the panel colour with the author, the age and a Review's state chip, green for an
        approval and red for changes requested, then the body.
      - A PR's state words are 11.5 px, joined by a muted "·": a Review request in the accent colour at
        weight 600, failed CI in the danger colour and passed CI in green.
      - In a description or comment, code sits on the panel colour, a quote has a muted line on its left,
        images keep inside the pane and emoji images are as tall as the text. Images show from the hosts
        GitHub serves them from, and from no other ([ADR 0005](../adr/0005-token-stays-in-the-rust-core.md)):
        badges and other third-party images through camo, custom emoji, uploaded images and the images of
        a Repository. An image with a variant for each side, such as CodSpeed's button, shows the one for
        the side of the theme in force. An uploaded image's address works for five minutes after the fetch
        that brought it, so an Issue or PR fetched before that, opened offline or before its first refresh,
        shows a broken image in its place until a fetch brings a fresh address. A refresh that changes only
        those addresses leaves the text as it is, with its sections open and its selection.
      - The tabs are 12 px, with the open one underlined in the accent colour.

    Esc closes what is on top, wherever the focus is: a dialog, the palette or a Peek first, then the wait
    for a chord's second key, a Definition peek, in the diff or the file view, or the diff's search, then the
    file view, which goes back to the PR's diff, and last a PR, which goes back to the Issue it was opened
    under, or else the first its view names. With nothing on top, Esc in the Detail pane gives the focus back
    to the Issue list, on the row of the item on show, or on the list itself when that item has no row, when
    the list shows; with the list
    hidden, Esc does nothing. A PR opened from a row of
    its own (a Standalone PR, one of its Backports, a Dependency update) has nothing under it, so Esc leaves
    it on show, as it leaves an Issue, and moves only the focus. A text box keeps Esc,
    unless it belongs to what Esc closes, like the search box. Esc gives the focus to whatever lay under the
    thing it closed: going back from the file view moves it to the diff, and going back from a PR moves the
    cursor and the focus to the Issue's row, or to the Detail pane while the Issue list is hidden. Esc leaves
    the panes as they are. Esc in the PR view's rail closes nothing: it gives the focus to the diff or the
    Conversation beside the rail ([review.md](review.md)).

    `F6` and `Shift+F6` move the focus through the panes on show, in the order Sidebar, Issue list, Detail
    pane, and back, skipping a pane that isn't shown and the Detail pane with nothing open
    ([keyboard.md](keyboard.md)). The pane that holds the keyboard focus, the Issue list or the Detail pane,
    shows a 2 px line in the Theme's focus colour along its top edge, as VS Code does, so the user can tell
    where ↑ ↓ go. With the focus in the Detail pane, as after a click on one of its buttons or a press of
    `F6`, ↑ ↓ scroll it a step at a time, `PgDn` and `PgUp` page it, and `Home` and `End` go to its start and
    end, even while the list shows: the diff on Files, the file view over it, the Conversation, or the code
    and hits of a Definition peek. With the focus on no control, as at launch, the list takes the keys and
    shows the line. While the
    list holds the focus, its row under the cursor takes the active selection colour, and the inactive one,
    a dim tint, once the focus moves away. The palette or a dialog on top, or another app in front, leaves
    every pane inactive. No ring goes round a whole pane.

    The palette and each dialog cover every pane, its sticky bars and icons included, under a dim scrim. A
    press on the scrim closes the palette or the dialog and doesn't reach the pane under it.

    The edges between the panes can be dragged; a double-click puts the default back. The Issue list has a
    minimum width, and so does the Detail pane, so in a narrow window the Issue list gives way first.

    `Ctrl+K Z`, or "Show Only the Detail Pane" in the command palette, hides the Sidebar and the Issue list
    for reading a long diff or thread, and remembers which of them showed. With neither on show, `Ctrl+K Z` or
    "Show the Panes" brings back the ones it remembers, or both when it remembers none. It is not a mode:
    `Ctrl+B` and `Ctrl+Alt+B` still bring back their own pane, and either of them makes `Ctrl+K Z` forget.
    The status bar stays, and the Issue list's rules for a hidden list apply. Bringing the panes back puts
    the focus where bringing back the list does; with the Sidebar alone, the focus stays in the Detail pane.

    Pane sizes, the width of the PR view's rail ([review.md](review.md)), the Definition peek's height,
    inline and split ([definitions.md](definitions.md)), the Sidebar's state, whether the Issue list shows
    and what `Ctrl+K Z` remembers belong to the machine, like the window's position, and are not synced.
    Whether the rail is open, and on which tab, lasts only until Tsugiki quits.

    "Reset Layout" in the command palette puts all of that back to the defaults at once, without asking: both
    pane sizes, the rail's 280 px, the peek's 18 rows and the split peek's 42%, the Sidebar full and on show,
    the Issue list on show, and nothing remembered by `Ctrl+K Z`. It leaves the rail open or closed.
    The status bar, the list's footer, zoom, the diff view, the performance HUD and the window stay as they
    are. It has no key until the user gives it one.

  - **Look** of the Sidebar and the Issue list, which the prototype settled. The colours come from the
    Theme's tokens ([themes.md](themes.md)).
    - The Sidebar has the panel colour and a line on its right. The Repository's name tops it in bold 13 px.
      Its section headings, "Built-in views", "Filters" and "Pinned", use the small heading style: 11 px,
      muted, upper case, 0.04 em apart. Its entries are 13 px with 4 px by 8 px padding and 4 px corners,
      with their counts muted at the right. The open view takes the selected tint at weight 600.
    - The list's header, 8 px by 10 px with a line under it, names the view. Each item is three lines of
      13 px, 7 px above and below, with a line between items: the people; the state icon, the number in the
      code font, the title, then the PR count and the age in muted 12 px; and the preview in muted 12 px.
      The state icon, and an Issue's fold chevron, centre on the title's line. An Issue with nothing waiting
      on the user counts its open PRs on the third line, or says it has none yet, and a PR gives its facts
      there. An ellipsis cuts a long title, people or preview.
    - An item that Needs attention looks like unread mail: a 3 px accent bar on its left edge, the title in
      the accent colour at weight 600, the people at weight 600 and the age in the accent colour.
    - The row under the cursor takes the selected tint, and a row under the pointer a light tint of the
      text colour. A click anywhere on a row opens it.
- The window's title names the open Repository, then the build: "python/cpython · Tsugiki 0.3.2 (6143447)",
  or "Tsugiki 0.3.2 (6143447)" on Onboarding. The Repository comes first, so the taskbar and Alt+Tab still
  show it when they cut the title short. Until the core says which build it is, the title reads "Tsugiki"
  after the Repository, and if the core doesn't answer, the status bar says so. Nothing on the screen names
  the build.
- A **status bar** runs along the bottom of every screen, below the panes. [status-bar.md](status-bar.md)
  says what each of its parts shows and when.
- A small floating panel sits in the window's bottom-right corner and says what the last action on an item
  did ([#1072](https://github.com/tsugiki/tsugiki/issues/1072)). It sits above the status bar at the height
  the bar draws, or at the window's bottom edge while the bar draws nothing, over the rail's foot while the
  rail is open. While the performance HUD shows, it sits 8 px above the status bar at the height it draws
  and the panel sits above the HUD. The panel takes no click, wheel or focus, so what it floats over still
  takes them, and the pointer on it only holds its countdown. With no Detail pane open, the Issue list ends
  above the panel wherever it sits, above the bar, the window's edge or the HUD, so the panel stays clear of
  its rows, the cursor's included. [workbench.md](workbench.md) says which actions show it and how it behaves.
