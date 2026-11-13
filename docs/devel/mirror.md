# Mirror

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A Repository's **Mirror** is first cloned as the Repository is added, alongside its first sync, one
  Repository at a time with the open one first. A switch puts the Repository's clone next, after the clone
  under way, which goes on to its end since git can't resume a clone cut off. A clone waiting for the user's
  answer waits on, and goes next once answered. Add repository offers to borrow objects from a clone the user
  has, picked as a folder, and gives the time and disk each choice takes. Tsugiki checks that the folder is a
  clone with a remote for that repository, and doesn't search the disk for one. The path is kept in
  `machine.toml`, so a new clone after a broken one borrows again. A Repository added before Tsugiki asked
  waits for its clone until the user answers, once on each machine. Its PRs' Files tab asks, with the same
  choices, times and disk as Add repository. The status bar says a mirror waits for an answer, and a click on
  that line asks in a dialog. A clone that fails is deleted and tried again, and one cut off by quitting
  starts again at the next launch. A clone git finished is kept, though quitting came as it ended or another
  program held a file as Tsugiki moved it into place.
- Tsugiki keeps each mirror in shape itself, and never changes the user's git config. Each day it deletes the
  refs of PRs the Workbench no longer reaches, apart from those opened since launch, and runs git's
  incremental maintenance; each week it runs `git gc`. A mirror git says is broken is moved aside and cloned
  again, borrowing again where it can, while the status bar shows the clone. Its PRs' heads are then fetched
  again.
- Each mirror keeps the commit the user's **Review** of each PR covered, under a ref of its own, so a Stale
  review check finds both sides there. That Review is the user's latest submitted one in any state, a
  dismissed one included, and the commit comes with the Conversation read ahead for PRs with the user's
  Review. The daily upkeep leaves these refs in place, and one moves only when the user's Review moves to
  another commit. A reviewed commit the mirror lacks, from a Review older than the mirror or on a commit a
  force-push dropped before the mirror fetched it, is fetched by its SHA, once. A commit GitHub refuses to
  send is noted beside the mirror and not asked for again.
- Between its git jobs the Mirror worker reads the per-file patch-ids of the reviewed commit and the head
  that the Stale review check compares, once the mirror has both, and the session keeps them in `tsugiki.db`.
  It reads only the commits the database lacks, so a save that failed is read again at the next sync. The
  diff behind them sets the user's diff settings back to git's defaults, so a change to those leaves each
  patch-id as it was. The worker also tells the session which of those commits it can't read and why, each
  time that changes, and reads none with a git older than 2.39.
- While a Repository's mirror is still being cloned, the diff, the definitions and Open in editor say how far
  it has got. The Conversation tab works meanwhile.
- Offline, Tsugiki starts no clone and no fetch, though upkeep, which needs no network, goes on. A clone or
  fetch under way goes on until it ends. A clone that waits for GitHub says so in the status bar, the Files
  tab and the Repository picker, and starts once Tsugiki is back online, as do the fetches held back.
- Without git 2.30 or newer, or without room on the disk for the clone, the diff says what is missing, with a
  link to git's downloads or the space needed, and offers Check again. The rest of Tsugiki works without it.
- Each Repository has a **Lookup checkout**, a detached worktree of the mirror in
  `repos\<owner>__<name>\lookup\`, which the Mirror worker makes on the first PR whose Files tab shows and
  moves to each PR's base (the merge-base) as its diff shows, without asking. The PR's added files are
  written into it too, and the next move removes them; files the PR changes or deletes stay at their base
  text. A move runs in the background and goes ahead of the worker's other git jobs, but waits for the one
  under way. PRs opened in quick succession don't queue moves: the next move goes to the PR on screen. A
  lookup or a search asked during a move waits for it to end, and one asked where git is missing hears at
  once that there is no checkout. A move needs no network, so offline it goes to any base the mirror holds.
  git's gc keeps the commit the checkout is on. A move runs no hooks and no filter such as git-lfs's,
  and leaves line endings as the mirror has them, whatever the user's `core.autocrlf`. A move
  that fails leaves no checkout and isn't tried again for the same PR until a lookup asks, when it makes
  the checkout afresh. Short of disk, a move resets a checkout that is there but doesn't make one. Until its
  mirror is there, a lookup hears at once that there is no checkout. It lives in the Repository's folder, as
  does git's note of it in the mirror, so removing the Repository removes it
  ([ADR 0020](../adr/0020-go-to-definition-asks-a-language-server-which-reads-a-lookup-checkout.md)).
- "Open in editor" runs the editor command (`code -g` by default) on the **Review checkout**, moved to the
  PR's current head. If the user has edited files there, Tsugiki first offers Discard and switch, Copy as
  patch, or Cancel. Settings › Repositories shows the checkout's size, with Remove checkout.
