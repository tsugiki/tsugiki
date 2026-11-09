# Changelog

Release notes for Tsugiki. Keep "WIP" current as changes land, and rename it to
`## Version X.Y.Z (YYYY-MM-DD)` in the PR that sets the version. The release workflow uses that section as the
release's body, and fails without it
([ADR 0012](docs/adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## WIP

### Fixed

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
