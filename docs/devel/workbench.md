# Workbench

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The **Workbench** lists each **Issue** that a **Filter** switched on the Workbench reaches, each **Pinned
  issue**, and each Issue with an open PR the user holds a **Review** on. An Issue being closed on GitHub
  does not remove it.
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
- Adding a Repository, Maintained or not, creates two starter Filters, ordinary ones the user can edit or
  delete: "Mine" (`is:open author:@me`), switched on, and "Involved" (`is:open involves:@me -author:@me`),
  switched off. Repositories added before the switch keep their Filters, switched off, and so does a
  Repository whose settings leave its Filters out: it reads as having both starters, switched off.
- Clicking a person's name opens a temporary `author:<login>` Filter. It lists their Issues and the Issues
  their PRs fix. It stays in the Sidebar, marked unsaved, until the user saves it as a Filter or closes it.
- **All issues** is an unsaved Filter over the Repository's open Issues and open PRs, newest update first,
  with a search box that takes GitHub search syntax; Save turns a search into a Filter. Each PR is a row of
  its own, and every row in every Issue list has an Issue or PR icon coloured by state. Two quick filters,
  **Issues** and **PRs**, each with its count, show one kind or both (at least one stays on; also in the
  command palette). From it the user reads, comments on and pins Issues and opens PRs. It shows what changed
  since the last sync first and loads older items only as the user scrolls or searches.
- The Sidebar shows counts the way a mail client shows unread counts. For the Workbench, the Filters switched
  on and the Pinned issues, the count is how many Issues Need attention. For a Filter switched off, it is how
  many items its search found, as its view lists them. For the Built-in views, it is how many PRs they hold.
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
  - on any item, followed Issues included: the user **Marked unseen** it.
- Opening an item leaves its Last seen alone, so it keeps its place in the list while the user reads it.
  - **Mark Seen** moves Last seen to the moment of the item's last sync that the window drew, never back,
    and clears Marked unseen. On an Issue it marks the Issue and every PR in its tree; on a PR, that PR. It
    clears only what Last seen measures: a Review request, a Stale review or failed CI stays.
  - **Mark Unseen** flags the item Marked unseen. It then Needs attention from the moment of the flag, so By
    attention sorts it first, and its preview says "Marked unseen". Flagging a PR brings its Issue back with
    it. Tsugiki keeps the flag per Repository and item.
- An item with nothing that Needs attention, such as a Pinned issue or the user's Review on a PR nobody
  touched, stays in the one list: By attention sorts it below the items that do, and By last update among
  them. It has no section or fold of its own, in the Workbench, a Filter or a Built-in view.
- A PR that fixes no Issue is a **Standalone PR**: its own item in the Issue list.
- A Pinned issue stays until the user unpins it.
- Hiding, all kept in Tsugiki only:
  - **Archive** puts an item away until something new needs attention there.
  - **Mute** hides an item until the user unmutes it.
  - **Mute author** hides the Issues and Standalone PRs a person or bot opened, in the current Repository
    only. Their PRs inside a followed Issue stay listed without needing attention, and an Issue they opened
    still shows when another PR in it needs attention or a Filter matches it.
  - A Review request or an assignment to the user always shows, even when Muted.

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
