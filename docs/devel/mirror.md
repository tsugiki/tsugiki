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
- While a Repository's mirror is still being cloned, the diff, the definitions and Open in editor say how far
  it has got. The Conversation tab works meanwhile.
- Without git 2.30 or newer, or without room on the disk for the clone, the diff says what is missing, with a
  link to git's downloads or the space needed, and offers Check again. The rest of Tsugiki works without it.
- "Open in editor" runs the editor command (`code -g` by default) on the **Review checkout**, moved to the
  PR's current head. If the user has edited files there, Tsugiki first offers Discard and switch, Copy as
  patch, or Cancel. Settings › Repositories shows the checkout's size, with Remove checkout.
