# Workbench

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The **Workbench** lists each **Issue** that a **Filter** switched on the Workbench reaches, each **Pinned
  issue**, and each Issue with an open PR the user holds a **Review** on. An Issue that closes on GitHub
  leaves it for Archived, as "Closing archives" below says.
- **Filters** are written in raw GitHub search syntax, with `repo:` of the current Repository added
  implicitly. They run with GitHub's advanced search, which is the only mode that understands `OR` and
  parentheses.
- Each Filter has an "On the Workbench" switch, `on_workbench` in `settings.toml`, off unless the file says
  so ([#819](https://github.com/tsugiki/tsugiki/issues/819)). The Filter's view has it beside its name, and a
  change there saves it at once. A change made while another saves waits for it, then saves the switch as
  the user last set it, and the switch stays in focus throughout. The items of a Filter switched off
  stay out of the Workbench and out of **Reach**: Tsugiki fetches no head and reads ahead no Conversation for
  them. The sync still runs its search, so its view lists what it found, its **Matched PRs** first. In an
  Issue view, a PR any Filter found, switched on or off, stays out of the fold.
- Switching a Filter on puts its items in Reach at once: the Mirror fetches the heads of the PRs it newly
  reaches, and the Repository's next refresh reads ahead the Conversation of each of those PRs that Tsugiki
  hasn't read at the PR's head, while the PR is still in Reach. Switching one off stops the Mirror
  wanting the heads of the PRs it alone reached, and the refresh reads none of them ahead.
- A Filter's view goes by its search, so a Repository holds one Filter for each search: of two with the
  same search, Tsugiki reads the first.
- A Filter's view lists everything its search found, switched on or off, as a mail client's search finds
  archived and muted mail ([#824](https://github.com/tsugiki/tsugiki/issues/824)). An Archived item stays in
  its place, marked Archived and not needing attention. Muted items, and those of **Muted authors**, go at
  the end in a fold, "Show 4 muted", which a click opens and "Hide 4 muted" closes, and each view keeps
  its fold open or closed while the Repository stays open. The cursor goes into the fold only while it
  shows, and putting the cursor on an item in it, as going back to it does, opens it. A Muted item stays
  in its place, marked Muted, while a Review request or an assignment to the user stands in its tree, as
  Mute below says. The Filter's count leaves out its Archived and Muted items, and a Filter switched on
  brings none of them onto the Workbench. Reach stays as Mute and Archive below say.
- Adding a Repository, Maintained or not, creates two starter Filters, ordinary ones the user can edit or
  delete: "Mine" (`is:open author:@me`), switched on, and "Involved" (`is:open involves:@me -author:@me`),
  switched off. Repositories added before the switch keep their Filters, switched off, and so does a
  Repository whose settings leave its Filters out: it reads as having both starters, switched off.
- Clicking a person's name opens a temporary `author:<login>` Filter. It lists their Issues and the Issues
  their PRs fix. It stays in the Sidebar, marked unsaved, until the user saves it as a Filter or closes it.
- The **Inbox** lists every open item of the Repository the user hasn't triaged: each open Issue with its PRs
  under it, grouped as on the Workbench, and each Standalone PR on its own, the latest update first. It has
  no Issues or PRs switch, and no switch of order. Pin, Archive and Mute each take an item out, and so does
  the item closing, at the next refresh. A PR that opens on an Archived Issue brings the Issue back, closed
  or not, as "A PR that opens on an Archived Issue" below says. **Muted authors**' items stay out unless
  the user turns on "Show muted". It starts with the items updated in the last two days, and scrolling to
  the end of the list reads
  the next older batch from the cache. The window builds a row for each item of the batches read so far,
  and the browser lays out and paints only the rows on screen, so the Inbox of a cache the size of
  CPython's scrolls as a small one does. A change reads the batches on show again in one query, which
  ends where they ended. It adds nothing to **Reach**. A search box takes GitHub
  search syntax, and Save turns a search into a Filter. From it the user reads, comments on and pins Issues
  and opens PRs. "View: Inbox" in the command palette opens it.
- Every row in every Issue list has an Issue or PR icon coloured by state.
- The Sidebar shows counts the way a mail client shows unread counts. For the Workbench, the Filters switched
  on and the Pinned issues, the count is how many Issues Need attention. For a Filter switched off, it is how
  many items its search found, as its view lists them, less the Archived and Muted ones. For Stale reviews,
  it is how many PRs the view holds, and for Review requests and Archived, how many items. The Inbox has no
  count.
- **Built-in views**: Stale reviews, New activity, Review requests, Archived (the Archived items, the last
  archived first, each with Unarchive) and Muted (the Muted items with Unmute, then the Muted authors in this
  Repository and the items they hide, with Unmute author).
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
  - on any item, followed Issues included: the user **Marked unseen** it.
- Opening an item leaves its Last seen alone, so it keeps its place in the list while the user reads it.
  - **Mark Seen** moves Last seen to the moment of the item's last sync that the window drew, never back,
    and clears Marked unseen. On an Issue it marks the Issue and every PR in its tree; on a PR, that PR. It
    also clears a Review request, a Stale review and failed CI on an Own PR that started before that
    moment, while a new request, a push that makes the Review stale again or a CI run that fails later
    counts again. A Stale review starts at the push or when the check found it against the PR's head,
    whichever came later, so one the window hadn't drawn when the user marked counts again. An item on the
    Workbench only because it Needed attention leaves it, and one that a Filter, a pin or the user's Review
    holds stays as a plain row. The Review requests and Stale reviews views keep listing their PRs.
  - **Mark Unseen** flags the item Marked unseen. It then Needs attention from the moment of the flag, so By
    attention sorts it first, and its preview says "Marked unseen". Flagging a PR brings its Issue back with
    it. Tsugiki keeps the flag per Repository and item.
- An item with nothing that Needs attention, such as a Pinned issue or the user's Review on a PR nobody
  touched, stays in the one list: By attention sorts it below the items that do, and By last update among
  them. It has no section or fold of its own, in the Workbench, a Filter or a Built-in view.
- A PR that fixes no Issue is a **Standalone PR**: its own item in the Issue list.
- A Pinned issue stays until the user unpins it.
- Hiding, all kept in Tsugiki only:
  - **Archive** puts an item away until something new needs attention there: an Issue, a Standalone PR or
    a Standalone Backport. It marks the item seen as Mark Seen does, across an Issue's whole tree, so a
    comment from before it brings nothing back. The item leaves the Workbench, the Inbox and the Built-in
    views, and their counts and the Filters' counts, apart from a PR whose Review request stood at the
    archive: the Review requests view lists it, marked Archived and not needing attention, until the request
    ends. Each Filter that matches it lists it in its place, marked Archived. A Review request, failed CI or
    a comment that needs attention after the archive brings the item back, no longer Archived, and Mark Seen
    then leaves it back; New activity that doesn't need attention, as on an Issue the user only follows,
    leaves it put away, and so does Mark Seen on an item the window shows Archived. Mark Unseen unarchives
    it.
    Tsugiki keeps it per Repository under a key that names the item, its kind and number, which Mute
    shares, with the moment the user put it away. The Archived view lists the items the last archived first:
    archiving an item again moves it to the top, and Mark Seen on it there leaves it in its place.
    Unarchive, or `E` on its row there, puts the item back wherever its state puts it. Mark Unseen on any of
    its rows there unarchives the item and flags that row's Issue or PR Marked unseen, so it comes back
    Needing attention.
  - **Closing archives** ([#820](https://github.com/tsugiki/tsugiki/issues/820)): an Issue or a Standalone
    PR that closes or merges on GitHub goes to Archived by itself, put away when the sync sees it close, if
    the Workbench or **Reach** held it while it was open, a Filter's find included. A closed Issue with an
    open PR, such as a Backport or the user's own PR, archives too. An open Issue keeps its merged and
    closed PRs in its tree. Closing marks nothing seen, so what needed attention at the closing keeps the
    item on the Workbench. A closed Archived item, however it got there, stays put away while nothing in its
    tree Needs attention, and comes back while something does; Mark Seen then puts it back in Archived. An
    item that reopens leaves Archived. Unarchive keeps a closed item out of Archived until it reopens and
    closes again. A Muted item stays Muted.
  - **A PR that opens on an Archived Issue** ([#823](https://github.com/tsugiki/tsugiki/issues/823)), such
    as a follow-up on an Issue closed long ago, unarchives the Issue as Unarchive does, and brings it back
    to the Inbox as a row with its PRs, closed or not, until the user archives or mutes it, or it reopens or
    closes. Mark Seen leaves it out of Archived. The new PR alone doesn't make the Issue Need attention, so
    the row shows the Issue as it stood, and the Issue is on the Workbench only if something else puts it
    there: a Filter that reaches closed Issues, an open PR in its tree the user holds a Review on, or Needs
    attention. A Backport carries an older change and brings nothing back, and a Muted Issue stays Muted.
    Only a PR that changed since the last refresh can open: one that a read of an Issue's tree, an older
    batch or an Origin PR loads from before opens nothing. A closed Issue stands in the Inbox at the latest
    update of its PRs.
  - **Mute** hides an item until the user unmutes it: an Issue, a Standalone PR or a Standalone Backport.
    It leaves the lists and counts an Archived item leaves, the Archived view too, whatever happens to it
    later, and each Filter that matches it lists it in the fold at the end of its view. Mute marks the item
    seen as Archive does and clears Marked unseen, and on a Muted item Mark Seen and Mark Unseen do nothing.
    Only someone asking the user by name gets through. While a Review request or an assignment to the user
    that stood at the mute stands, each Filter that matches the item lists it in its place, marked Muted and
    not needing attention, and so does the Review requests view for a PR. While one that started after the
    mute stands, the Workbench lists the item too, marked Muted, Needing attention for that alone, until the
    request or the assignment ends. An assignment's start is the newest the Issue's or PR's timeline holds;
    one the timeline no longer holds counts as from before the mute. Tsugiki keeps Muted items per
    Repository under Archive's key.
  - A Muted item's tree leaves **Reach** while nobody asks the user about it
    ([#918](https://github.com/tsugiki/tsugiki/issues/918)): the Mirror fetches none of its heads and
    Tsugiki reads ahead none of its Conversations, so opening one of its PRs from the Issue view or the
    history waits on a fetch. A PR in it that another item reaches through its links stays in Reach. A
    Review request or an assignment to the user anywhere in the tree, acknowledged or not, keeps the whole
    tree in Reach until it ends. Unmute puts the tree back at once, as switching a Filter on does: the
    Mirror fetches its heads, and the next refresh reads ahead the Conversations Tsugiki hasn't read at
    their heads. An Archived item stays in Reach, a closed one too.
  - **Mute author** ([#494](https://github.com/tsugiki/tsugiki/issues/494)) mutes the author of the row
    under the cursor, or of the Detail pane's item while it holds the focus, in the current Repository
    only: a PR's author on a PR's row, an Issue's on an Issue's. The command palette offers it, as
    "Unmute Author" on a Muted author's row, and so does the Issue view's button; it has no key, and it is
    off on the user's own items. A Muted author's Issues, Standalone PRs and Standalone Backports count as
    Muted, those they open later too: they leave the Workbench, the Archived view and the counts, each
    Filter lists them in its fold of Muted items, and their trees leave Reach as a Muted item's do. Muting
    marks their open items seen in the background, the trees they head included. It finds those items in
    the facts as it runs, not on a page the window drew, so unlike Mute on one item it acknowledges no
    Stale review: Unmute author brings back each one that stood at the mute, while the Review requests it
    cleared stay cleared. A Review request or an assignment to the user brings one back as it brings back a
    Muted item. `Q`, `Shift+Q`, `E` and `M` do nothing on their items; Unmute author is the way back, and
    leaves an item the user muted on its own Muted. Their PRs inside another author's Issue stay as they
    are. The Inbox lists their items until [#825](https://github.com/tsugiki/tsugiki/issues/825) leaves them
    out.

## Issue tree

- Each **Origin PR** shows its **Backports** beneath it, and every PR shows its state, CI status, and the
  user's review state.
- In the Issue view, an Origin PR that has merged, with none of its Backports open, folds into one line under
  the rest, "Show 152 merged PRs", which counts the Backports too. A Backport closed unmerged counts as done,
  as when a manual Backport replaced a bot's that conflicted. A Backport listed without its Origin PR folds
  once it has merged. A click unfolds them in place, and "Hide 152 merged PRs" folds them again; the line
  keeps its state while a PR from the tree is on show. An entry stays out of the fold when one of its PRs
  Needs attention or is a **Matched PR**.
- Only Backports that already exist are shown; Tsugiki does not try to work out which ones are missing.
- Trees are whole for the user's own Issues and any Issue they open: its older PRs and Backports, and a
  Backport's Origin PR, are fetched even when they fall outside the synced window. Another Issue's row can
  lack those older PRs until it's opened. Links follow GitHub alone; a wrong one is fixed on GitHub, not in
  Tsugiki ([ADR 0011](../adr/0011-pr-links-from-github-signals-by-repository-conventions.md)).
- **Draft PRs** get a badge.
- A PR that fixes several Issues shows under each of them, with "also fixes #N" on its row.
- **Stacked PRs**: a PR that targets another PR's branch is stacked on it. Its row and PR view say "stacked on
  #N", and the PR view lists the whole stack in order. Tsugiki reads "Closes #N" from a Stacked PR's
  description itself, because GitHub ignores it there. When the PR below has merged and a Stacked PR still
  targets its branch, Tsugiki warns that the Stacked PR will not reach the default branch.
