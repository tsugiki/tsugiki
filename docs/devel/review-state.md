# Review state and activity

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A PR's review state is **Unreviewed**, **Reviewed**, or **Stale review**.
- A **Review** is either a review submitted on GitHub (line comments included) or a **Local mark**. The
  latest one counts, in any state, a dismissed one included, as staleness goes by commit. A review that only
  replies to existing threads doesn't count.
- A Review becomes stale only when the PR's diff outside **Generated files** changes. A merge from `main`, or
  a rebase that leaves the diff the same, does not count. Rebases are detected by comparing patch-ids file by
  file, each side diffed against its merge-base with the Target branch. Every whitespace change counts,
  trailing spaces included (`git patch-id --verbatim`): indentation matters in Python, and a false "stale"
  costs only a look. Files whose change is whitespace only are named as such.
- Each open PR with the user's Review is checked in the background, once the Mirror worker has a head that
  differs from the reviewed commit. The worker reads both commits' per-file patch-ids between its git jobs,
  and `tsugiki.db` keeps them per commit and Target branch, so a second check of the same commits runs no
  git. A head equal to the reviewed commit is Reviewed with no check.
- While a check is pending, the PR keeps the verdict of its last check of the same Review, or Reviewed if it
  has none, so rows and counts don't jump while git runs. `tsugiki.db` keeps the two commits each PR's last
  check compared. A check that can't run gives a Stale review with the reason in the hover: the mirror not
  cloned yet, or cloned again as git said it was broken; no git, or one older than the 2.39 that
  `git patch-id --verbatim` needs; a reviewed commit GitHub refused to send. When the reason goes away, the
  check runs and its verdict replaces the reason.
- `tsugiki.db` keeps each PR's last finding that the user's Review is stale, or that its check can't run:
  the reviewed commit, the head, and when a check first found the two. A new head or a new Review makes a
  new finding. The finding ends once a check finds the diff the same, the head goes back to the reviewed
  commit, or the user reviews again. A check that waits and a restart keep it, and so does a PR that
  closes. Mark Seen, Archive and Mute acknowledge only the findings the window drew on the item, so a
  finding of a later head, or one the check made after the window drew the page, still counts. A Muted
  item draws none, as it Needs attention for a fresh ask alone, and Mute author acknowledges none.
- A merged or closed PR's check runs once on its final head, while the PR is in **Reach**, and the
  verdict freezes with the PR: once `tsugiki.db` holds both sides, later syncs and fetches read nothing of
  it again. A PR no longer in Reach by then isn't checked. Its row keeps the Stale review
  badge, with the changed files in the hover, but it never Needs attention and adds to no count. While its
  check waits or can't run, it shows what an open PR's would. A PR merged with a merge commit has its
  commits on the Target branch, so each one's diff starts from the branch as it stood before that merge.
- The check runs only on PRs in Reach, so a **Muted** item's PRs go unchecked while nobody asks the user
  about it ([#918](https://github.com/tsugiki/tsugiki/issues/918)). A PR that Unmute puts back in Reach
  keeps its last verdict until the Mirror has fetched its head and the check has run.
- A **Bot backport** shares its Origin PR's review state, so Tsugiki checks none of its own. A **Manual
  backport** is reviewed on its own, and so is a Bot backport whose Origin PR Tsugiki lacks or is a Backport
  too, which the list shows by itself.
- A **Review request** counts as Needs attention until the user marks the PR seen, including on a Bot
  backport or a Draft PR, and the Review requests view lists it until the user submits a review. A Local
  mark ends neither.
- A colleague's Draft PR only Needs attention when it has a Review request. When it becomes ready for review,
  that counts as **New activity**.
- A Bot backport's Stale review is counted once, on its Origin PR.
- **Last seen** is recorded per PR and per Issue. Mark Seen, Archive and Mute move it, and so will Unmute and
  a write to GitHub. Opening an item, opening it on GitHub or handing a PR to a review tool doesn't. It never
  moves back.
- **New activity** covers people's commits, comments and reviews, a failed Bot backport, and CI changes on an
  **Own PR** or on a PR the user holds a Review on. Other bot activity and label changes do not count.
- A Bot backport has failed when the bot says it could not make it, which counts on the Origin PR, or when
  its CI fails, which counts on it and on its Origin PR. A CI change means CI reached a result, so a push that
  sets CI back to pending is no news.
- On an item the user has never marked seen, only what happened since the Repository's first sync counts, so a
  first sync doesn't mark months of history as new.
