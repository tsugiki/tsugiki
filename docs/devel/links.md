# Links

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Links in descriptions and comments are sorted by where they point. Tsugiki never navigates its own window to
  a web page.
  - An Issue or PR in the current Repository opens in the Detail pane. `Alt+←` / `Alt+→` go back and forward
    through what was opened, and move the cursor and the focus to its row. A slim bar over the Detail pane
    shows ‹ and › (greyed out when there's nothing that way, the target in the tooltip) and the title of the
    item shown. Right-clicking an arrow lists the recent items that way; there is no other history manager.
    The mouse's back and forward buttons and the command palette (Go Back / Go Forward) do the same. History
    records only items opened in the Detail pane: switching views in the Sidebar adds nothing, and re-opening
    the item already shown adds nothing. Each Repository has its own history, which starts fresh when you
    switch to it.
  - An Issue or PR anywhere else on GitHub opens in a **Peek** popup that Tsugiki renders from the API: title,
    state, author, description, the last few comments, with Open in browser and, for an added Repository, Open
    in Tsugiki. GitHub can't be embedded in a frame.
  - Anything else opens in the system browser. `Ctrl`+click sends any link to the browser, and `Ctrl+Shift+G`,
    or a click on the title in the Detail pane, opens the current Issue or PR on GitHub (Dependency updates:
    the bot's open PRs there).
