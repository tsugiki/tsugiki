# Changelog

Release notes for Tsugiki. Keep "WIP" current as changes land, and rename it to
`## Version X.Y.Z (YYYY-MM-DD)` in the PR that sets the version. The release workflow uses that section as the
release's body, and fails without it
([ADR 0012](docs/adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## WIP

## Version 0.2.4 (2026-11-13)

You pick your Themes, and can write your own. An item stays in the Issue list until you mark it seen, and
the Files tab can show one file at a time.

### Added

- Gruvbox Light Hard and Gruvbox Dark Hard join the built-in Themes. `[appearance]` in `settings.toml`
  picks the mode, `theme = "system"`, `"light"` or `"dark"`, and one Theme for each side with
  `light_theme` and `dark_theme`. Light and Dark keep their side whatever Windows says, title bar included.
- `Ctrl+K T`, or "Pick a Theme" in the command palette, lists the Themes of the side in use. Each one the
  list moves to shows on the whole window, Enter saves it to `settings.toml`, and Esc puts the old one
  back. The last row lists the other side's Themes.
- Your own Themes are TOML files in a `themes` folder beside `settings.toml`. A file sets only the colours
  it changes and takes the rest from the Theme it `extends`, or from VS Code's Theme for its side.
  `Ctrl+K R`, "Reload Themes" in the palette, or opening the picker reads the folder again. The picker
  shows a file that can't be used greyed, with why, and the status bar names the file, the line and the
  key of each line Tsugiki skipped. `docs/devel/themes.md` describes the format.
- The Files tab can show one file at a time, with a bar over it that reads "‹ 12 of 48 ›". `Ctrl+K O`, the
  switch in the tab's header, or "View: One File at a Time" in the palette switches the PR on show, and
  `[diff] files = "one"` in `settings.toml` makes it the default. `Alt+PageDown` and `Alt+PageUp` go to
  the next and the previous file either way.
- Every file in the Files tab collapses to its header with a click on its chevron, as Generated files did.
  A jump into a collapsed file, with `F7`, `F8` or Find in diff, opens it.
- The Issue view shows the Issue's labels. An item cached before this version shows its labels once a sync
  reads it again.
- Mark Unseen, on `Shift+Q`, in the palette, or the button under the item's title, flags an item so it
  Needs attention again and its preview says "Marked unseen".

### Changed

- Opening an item, opening it on GitHub or handing a PR to a review tool no longer marks it seen, so it
  keeps its place in the Issue list. Mark Seen does that: `Q`, the palette, or the button under the item's
  title. On an Issue it marks every PR in its tree too, and the cursor moves to the next item that still
  Needs attention. A Review request, a Stale review or failed CI keeps an item in Needs attention.

## Version 0.2.3 (2026-11-12)

Tsugiki takes the look of its prototype and follows Windows to light or dark. The Issue list and the Issue
view fold what can wait, and a line shows which pane your keys go to.

### Added

- The window takes its colours from a Theme, VS Code Light Modern or Dark Modern, whichever matches the
  side Windows is on, and switches when Windows does. It opens in that Theme's background, with no white
  flash before the page loads.
- A line along the top of the Issue list or the Detail pane shows which one holds the keyboard focus. The
  row under the cursor takes the active selection colour while the list holds it. With the focus in the
  Detail pane, as after a click on one of its buttons, `↑` and `↓` scroll it.
- Back and forward arrows over the Detail pane walk its history, as `Alt+←` and `Alt+→` do, and their
  tooltips name where they go. A right-click on one lists up to 12 items that way. The mouse's back and
  forward buttons, and "Go Back" and "Go Forward" in the command palette, do the same.
- A header over the Issue list names the view on show. A switch at its end sorts the list "By attention" or
  "By last update", and so do "Sort by Attention" and "Sort by Last Update" in the palette. `machine.toml`
  keeps the choice as `order` under `[panes]`.
- The Issue view says when the Issue was opened, how many comments it has and who it's assigned to.
- An "Origin PRs" heading over the Issue view's PRs counts them, as "2 open of 5 PRs". Each PR names its
  target branch and its author, a PR that Needs attention shows in bold, and a legend under the tree says
  what the icons and the bold mean.

### Changed

- The Issue list folds each Issue's PRs, as a mail client folds a conversation. `→` or a click on `›`
  unfolds them and `←` folds them; on a PR's row, `←` moves the cursor to its Issue. Opening a PR some
  other way, such as with `Alt+←`, unfolds its Issue.
- The Issue list folds its quiet items behind "Show 4 quiet items", and "Show Quiet Items" in the palette
  unfolds them too. `↑` and `↓` skip a folded section.
- The Issue view folds the Origin PRs that merged, with no Backport left open, into one line: "Show 12
  merged PRs". A PR that Needs attention or matches one of your Filters stays out of the fold.
- The Issue list, the Sidebar, the Issue view and the PR view take the prototype's look. The UI and code
  each get one font, buttons and inputs share one style, each item in the Issue list takes three lines, and
  the PR tree, the description and each comment sit in boxes. A Review's state shows as a green or red chip.
  A click anywhere on a row of the Issue list or the PR tree opens it.

### Fixed

- The Sidebar, the Issue list and the Detail pane each scroll by themselves, with the scrollbar at their own
  right edge. Before, the window scrolled the Issue list, so the wheel over the Sidebar, or past the end of
  the Detail pane, scrolled the list.

## Version 0.2.2 (2026-11-10)

The status bar tells you when a newer Tsugiki is out, and one small crate holds the only code that can read
your GitHub token.

### Added

- Tsugiki asks GitHub Releases for its latest release at start-up and every 6 hours. If that release is
  newer than yours, the status bar says "Tsugiki 0.3.0 is out", and a click opens the release page in your
  browser. The request carries no token. A failed check leaves the status bar alone and writes a line to
  `tsugiki.log`.
- `check = false` under `[releases]` in `settings.toml` turns the check off from the next start, and Tsugiki
  then sends Releases nothing.

### Changed

- A crate of its own holds the GitHub CLI's token and is the only code that can read it. The token goes out
  in requests to `api.github.com` and nowhere else.

## Version 0.2.1 (2026-11-10)

You can hide the Sidebar and the Issue list, show the Detail pane alone, and put the layout back.
`Ctrl+R` opens a picker that switches between your Repositories, and the window title names the open one.

### Added

- `Ctrl+B` hides the Sidebar and `Ctrl+Alt+B` hides the Issue list, and the same key brings the pane back at
  its width. With the list hidden, the Detail pane takes its room and the focus, and `↑` and `↓` scroll it.
  A list that comes back puts the cursor and the focus on the row of what the Detail pane shows.
- `Ctrl+K Z` hides both panes, and pressing it again brings back the ones it hid. `Ctrl+K` starts a chord:
  the status bar shows it until you press the second key, and `Esc` or a click ends the wait.
- The command palette offers each of these, as "Hide Sidebar", "Hide Issue List" and "Show Only the Detail
  Pane", and "Reset Layout" gives both panes their default widths and shows them. `machine.toml` keeps which
  panes show, beside their widths.
- `Ctrl+R`, or a click on the Repository's name at the top of the Sidebar, opens the Repository picker. It
  lists your Repositories in the order you last opened them, and picks the one you had open before,
  so `Ctrl+R` and `Enter` go back to it. A row says why that Repository's diffs aren't there yet, such as
  "cloning, 40%" or "the mirror waits for your answer". The palette also lists "Repository: owner/name" for
  each one but the open one.
- A switch opens the Repository as a launch does, and its Mirror clones ahead of the others waiting. The next
  launch opens the Repository you opened last.
- Add repository on a Repository Tsugiki already has offers "Switch to it" in place of Add, and saves
  nothing.
- The window title names the open Repository, as "python/cpython · Tsugiki", so the taskbar and `Alt+Tab`
  show it.
- The About line shows the commit a build came from beside its version, as "0.2.1 (3a6dc89)", and so do
  the first line of `tsugiki.log` and the performance HUD's recordings.

### Changed

- `machine.toml` keeps the Repositories you opened as `recent`, newest first, in place of
  `last_repository`. 0.2.1 reopens the Repository a 0.2.0 file names, and turns `last_repository` into
  `recent` the next time it writes the file.

### Fixed

- The Files tab of a Repository whose Mirror waits for your answer asks where the Mirror gets its objects,
  and its button starts the clone. Before, it said it was cloning, and the question waited in the status bar.
- `Esc` on a PR with no Issue under it, such as a Standalone PR, leaves the PR on show. It used to empty the
  Detail pane.
- Going back from a PR to its Issue, with `Esc`, the gh-N link, `Alt+←` or `Alt+→`, moves the focus to the
  Issue's row. Before, the focus stayed on the PR's row, and `Enter` opened the PR again.
- A click outside the palette, Add repository or the mirror question closes it and gives the focus back to
  where it was. Before, the browser then moved the focus to the page itself.
- A drag of a pane's edge right after launch, before Tsugiki had read `machine.toml`, no longer saves the
  default widths over yours.
- On macOS, a git helper that started as Tsugiki stopped git could go on running after it.

## Version 0.2.0 (2026-11-09)

A PR opens in the Detail pane, with its Conversation and a highlighted diff.
Each Repository gets a Mirror, a bare git clone that Tsugiki reads the diff from, so you need git 2.30+.
Issues and PRs show GitHub's Octicons for their state.

### Added

- A PR opens in the Detail pane, on a Conversation tab with its description and recent comments, and a Files
  tab with its diff. The arrows stop on each PR and Backport in an Issue's tree.
  `Enter` or a click opens one, as does a PR in the Issue view, and `Esc` or the `gh-N` link goes back to the
  Issue, scrolled where you left it. `Ctrl+PageDown` and `Ctrl+PageUp` switch tabs, and so do `Enter` and a
  click on the open PR's line.
- Each Repository gets a **Mirror**, a bare git clone in its folder, which the Files tab reads diffs from.
  Adding a Repository starts its clone beside its first sync, and Repositories added before get theirs at the
  next launch, the open one first. The status bar shows "Cloning python/cpython 42%" beside the sync time.
  Quitting stops a clone, and the next launch starts it again. Tsugiki needs git 2.30 or newer on the `PATH`
  for this.
- Add repository asks where the Mirror gets its objects: downloaded from GitHub, or borrowed from a clone you
  already have. Each choice shows how long it should take and the disk the Mirror takes.
  Tsugiki only reads the clone, and the Mirror doesn't need it afterwards. For a Repository added before,
  the status bar asks once on each machine. `machine.toml` keeps the answer.
- Without git 2.30 or newer, or without room on the disk, Tsugiki makes no Mirror, and the status bar says
  why, with Check again. A clone that fails tries again a minute later, and waits twice as long after each
  failure in a row, up to about an hour.
- Tsugiki looks after each Mirror while no fetch waits. Each day it drops the branches of PRs your work no
  longer reaches and runs `git maintenance`, and each week it runs `git gc`. Tsugiki never runs
  `git maintenance start`, so your global git config stays as it was. When git says a Mirror is broken,
  Tsugiki clones it again.
- Each sync fetches into the mirror the branch of each PR your work reaches: your Review requests, your own
  PRs and those you reviewed, your Filters' PRs and the PRs in your Issues' trees. One `git fetch` takes all
  the PRs whose branch moved, and a PR GitHub has dropped fails nothing. While Tsugiki is offline the mirror
  waits.
- A PR's Conversation shows its Reviews among the comments, oldest first. Each Review is a block with its
  author, state, commit and body, and a line for each thread it started: the file and line, the first words,
  the replies, and whether it's Resolved or Outdated. A Review that says nothing beyond its state, such as a
  bare approval, takes one line. A comment hidden on GitHub folds to its reason, and a click opens it. If
  GitHub can't send the Conversation, the PR view says why.
- A refresh reads the Conversation of each changed PR your work reaches, so its Reviews are there when you
  open it. Each PR costs about one point of GitHub's hourly 5,000.
- The Files tab shows a PR's diff, read from its Mirror, so it asks GitHub for nothing. A PR moved to
  another Target branch shows its diff against the new one. A merged PR shows the diff GitHub shows,
  whether it merged with a merge commit, a squash or a fast-forward. The tab reads the diff the first time
  it shows, so stepping through PRs on their Conversations runs no `git diff`. Opening a PR still starts
  fetching a head the Mirror lacks.
- The diff highlights C, Python, Rust, TypeScript, JavaScript, Svelte, JSON, CSS, HTML, Markdown, reST, TOML,
  YAML and Makefiles, and code inside other code, such as a Markdown fence or an HTML `<script>`. Tsugiki
  parses a PR's files ahead, so a jump to another file finds it highlighted.
- Generated files start collapsed to their headers, and a click opens one. The Files tab renders the NEWS
  entries a PR adds above the diff, with Sphinx's roles as CPython's docs show them.
- Buttons on a hunk's row show 20 more lines above or below it, the rest of a short gap, or the whole file.
- `Ctrl+Alt+U` switches between the unified and side-by-side views, and the view holds for every PR in the
  Repository.
- Review threads sit under their lines in the diff. A resolved thread folds to one line, and `F8` and
  `Shift+F8` move through the open ones. An Outdated thread sits at the top of its file with the last lines
  of the hunk it was written against, and a thread on a line the diff leaves out opens the gap to it.
  Threads on files the diff no longer holds come last. Your Pending review's comments show in their
  threads, marked as yours, and a thread's line in the Conversation opens it in Files.
- A PR whose head has moved past the Mirror's copy shows the older diff, says it's fetching the newer one,
  and swaps it in at the same file. If that fetch fails, the tab says why, keeps the older diff and offers
  "Try again". So it does when GitHub's copy of the PR's branch still isn't at that head 10 s later, rather
  than saying it's fetching until the next sync.
- `F7` and `Shift+F7` move between the diff's changes. `Ctrl+F` finds text in the diff, the rows not drawn
  yet included. `Alt+←` and `Alt+→` go back and forward through the Issues and PRs you opened.
- The edges between the panes drag, and a focused edge moves with the arrow keys. A double-click puts the
  default width back. The widths stay on this machine, in `machine.toml`.
- Each Issue and PR shows GitHub's Octicon for its state, in GitHub's colors.

### Database

0.2.0 moves the database forward and leaves a copy of the old one beside it, as `tsugiki.db.v12.bak`. 0.1
won't open the new file: to go back, put the copy in its place. Tsugiki doesn't sync the cache again, though
the first sync fetches the state of the Issues 0.1 stored. A PR that 0.1 stored as merged by a fast-forward
shows an empty Files tab, since Tsugiki doesn't fetch a merged PR again.

## Version 0.1.1 (2026-11-07)

Repositories sync in turns, so a first sync of a large Repository no longer holds back the one you have open.
The window logs each performance budget it misses to `tsugiki.log`.

### Added

- Tsugiki writes a line to `tsugiki.log` each time the window misses a performance budget, such as a slow
  view query, a slow key or a long frame, and the start-up time of each launch that opens on a Workbench.
  Daily use then shows where Tsugiki is slow, without a recording from the performance HUD. A burst writes at
  most 10 lines of a kind a minute.

### Fixed

- Repositories take turns to sync, so a big first sync no longer holds back the others. The open Repository
  gets every other step. Before, a refresh waited behind a first sync of CPython, and a new PR took 12
  minutes to show. While another Repository syncs, the status bar names it beside the sync time.

## Version 0.1.0 (2026-11-06)

The first version for daily work. It lists the Issues that wait on you in `python/cpython` and your own
Repositories, and you answer them on GitHub.

### Added

- Add a Repository by its name or its link. Tsugiki uses the GitHub CLI's token, so sign in with
  `gh auth login` first.
- A first sync loads what waits on you before anything else, so the Workbench is right in about a minute.
  Then it loads everything open in a small Repository, or the last 30 days of a large one like CPython. The
  status bar shows its progress, and a sync you quit resumes where it stopped. After that, Tsugiki refreshes
  on its own, and when you click the status bar.
- The Sidebar: the Workbench, Review requests, the starter Filters, and Filters you write in `settings.toml`,
  each with its count.
- The Workbench lists Issues by Needs attention, says why each one waits on you, and shows its Origin PRs with
  their Backports beneath them.
- The Issue view: the title, description and comments, and each PR with its state, CI and Reviews.
- Last seen and New activity: an Issue you've read stops looking unread until something new happens on it.
- `↑`/`↓` and `Enter` move through the list and open an Issue, and `Ctrl+Shift+G` opens it on GitHub. On
  macOS, `Cmd` replaces `Ctrl` in Tsugiki's keys.
- The window shows whether Tsugiki can reach GitHub, and who is signed in.
- `tsg` runs one Tsugiki and passes `tsugiki://` links and other arguments to it.
- Print, Reload and the other browser shortcuts stay off, so Tsugiki gets those keys.
- A portable `.zip` for Windows and a `.dmg` for Apple silicon Macs. An installer comes later, with Alerts.
- A performance HUD, on `Ctrl+Shift+H` or from the command palette. It shows the window's speed against
  its budgets, names what came before a long frame, and times opening an Issue, from the database and from
  GitHub. Hide it to write the recording's numbers to `tsugiki.log`.

### First install

Tsugiki isn't code-signed yet, so Windows and macOS warn you the first time you open it. To make sure a
download is the file Tsugiki's workflow built, follow
[Checking a download](https://github.com/tsugiki/tsugiki#checking-a-download) before you open it.

- **Windows:** unzip `Tsugiki_0.1.0_x64-portable.zip` anywhere and run `tsg.exe`. When Windows says "Windows
  protected your PC", click More info, then Run anyway. Tsugiki keeps its data in the `data` folder next to
  `tsg.exe`.
- **macOS:** open the `.dmg` and drag Tsugiki to Applications. macOS blocks it the first time, so open
  System Settings › Privacy & Security and click Open Anyway beside Tsugiki.
