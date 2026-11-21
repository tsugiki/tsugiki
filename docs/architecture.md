# Architecture

This page says where things are and how they talk to each other. The reasons are in the
[ADRs](adr/), and the words in **bold** are defined in [CONTEXT.md](../CONTEXT.md). It names modules and
types, not functions, so it stays true longer. Update it in the PR that moves a module or adds a thread.

## The shape

Tsugiki is a Tauri app with five crates and a webview
([ADR 0007](adr/0007-core-library-and-thin-tauri-app.md)), and a sixth crate serves only the tests:

- `crates/core` (`tsugiki-core`) knows and decides everything: GitHub, the database, the **Mirrors**, the
  diff and every rule the screens show. It has no Tauri and no webview, and plain `cargo test` tests it.
- `crates/token` (`tsugiki-token`) holds the GitHub token, `gh` and the only HTTP client, and hides tokens
  in text ([ADR 0019](adr/0019-the-token-and-the-only-code-that-sends-it-have-a-crate-of-their-own.md)).
  Nothing outside it can read the token's string. Its [README](../crates/token/README.md) says what it does
  for readers who don't read Rust.
- `crates/lsp` (`tsugiki-lsp`) talks to a language server over its standard streams, for Go to definition
  (ADR 0020). It knows nothing of PRs, Mirrors or the database: the core says which server to start, which
  documents to open and what to ask. One thread owns each server, one writes to it, two more read its
  output and its error output, and requests reach the owner as messages. Its
  [README](../crates/lsp/README.md) says what it does and what it leaves to the core. Its `testing` feature
  also builds `tsugiki-fake-server`, a fake server the core's tests start as a process.
- `crates/app` (`tsugiki-app`) is the Tauri app. It starts the core's threads, turns commands and events
  into calls on the core, and owns the window.
- `crates/webview-keys` holds the one `unsafe` call that turns off WebView2's browser shortcuts
  ([ADR 0013](adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)).
- `crates/test-git` holds the test helpers that several crates share and that use no crate's types. It runs
  git and builds bare remotes and mirrors, gives the tests their folders under `.temp`, which go when a test
  passes, names the language servers they start, and checks generated files. Only tests depend on it. The
  helpers that use a crate's types are in its `testing` module, which other crates' tests reach through its
  `testing` feature.
- `ui/` is a SvelteKit single-page app that renders what the core computed.

The GitHub token never leaves the core ([ADR 0005](adr/0005-token-stays-in-the-rust-core.md)). The webview
gets view models, ready to render, and asks again when the core says something changed
([ADR 0008](adr/0008-view-models-and-invalidation-over-ipc.md)).

## Threads

Each piece of shared data has one thread that writes it. Other threads send that thread messages, or read
a copy through `latest::channel` (see the Rust design rules in [AGENTS.md](../AGENTS.md)).

```mermaid
flowchart LR
    gh[GitHub] <-->|GraphQL, one request at a time| github
    app -->|each screen's call, the user's actions| calls[Calls<br>a handle, no thread]
    calls -->|the user's demands: Repository and PR opened, the diff's ends, refresh, marks| fetching
    session -->|the rules' wants: heads, reviewed commits, patch-ids; heads saved| fetching
    github -->|facts saved, a Conversation's head| fetching
    fetching[fetching<br>a handle, no thread] -->|the session's queue| session
    fetching -->|git's queue| mirrors
    session[Session thread<br>marks, patch-ids, settings changes] -->|demands on GitHub, the user's first| github
    session -->|writes| store[Store thread<br>the only writer]
    github[GitHub thread<br>token, client, gate, sync queue] -->|writes| store
    store --> db[(tsugiki.db)]
    db --> reader[StoreReader thread<br>read-only]
    mirrors[Mirror worker] -->|patch-ids read, commits it can't read, to keep| session
    mirrors -->|a Repository's size, before a clone| github
    mirrors -->|git CLI| git[("repos/*/mirror.git<br>and lookup/")]
    github -->|status, activity, points, request in flight, last syncs, changes| board[StatusBoard and<br>mirror Announce callbacks]
    session -->|the changes its saves make| board
    mirrors -->|cloning, problems, each Mirror's state, held back| board
    board -->|latest copies + events| app[Commands in crates/app]
    reader --> calls
    git --> highlighter[Highlighter thread]
    git --> calls
    highlighter --> calls
    calls -->|what each screen shows| app
    settings[Settings owner<br>settings.toml, machine.toml] --> app
    settings --> calls
    calls -->|the PR on screen, lookups| definitions[Language servers thread<br>a ty per Repository]
    definitions -->|where the Lookup checkout is| mirrors
    definitions <-->|LSP over its standard streams| ty[ty]
    app -->|queries, each with its diff's ends, and cancels| search[Text search thread]
    search -->|where the Lookup checkout is| mirrors
    search -->|latest results| app
    gh -->|latest release, no token| releases[Release check]
    releases -->|a newer version| app
    app <-->|commands, events| ui[Webview]
```

| Thread | Owns | Where |
|---|---|---|
| Session | The Repositories the window opened since launch, and what the rules make of the stored facts: the Mirror worker's wants and the kept checks of the Stale review check. Takes each message at once: saves the marks and the patch-ids the Mirror worker read itself, follows each change to the settings, works out the wants again after the GitHub thread saves facts, and passes each demand on GitHub to the GitHub thread's queue. It answers at once a Fetch #N while GitHub can't be reached or the rate limit's gate holds, and a Refresh while one it passed still waits or runs, from `latest` copies of the status board's state and of the refreshes the GitHub thread finished. When the store kept its links under rules other than the set the build runs, it works them out again a batch of a Repository's PRs at a time while no message waits, the Repository on screen first (`session/refill.rs`). | `core/src/session.rs` |
| GitHub | The token, the GitHub client with its clock and the hour's points it sums from each answer's cost, the rate limit's gate, and the sync queue. Runs the syncs and the demands with blocking calls, one request at a time, takes the user's demands ahead of the next sync step, and answers the Mirror worker's question of a Repository's size. Reads the Issue or PR on screen again each minute while the user can see it, and reads ahead the Conversations the rules want. It writes the status, the sync's activity, the hour's points, the request in flight and the last syncs. The client announces the request in flight itself, as the thread waits while it is out. | `core/src/session/github_thread.rs` |
| Store | The one writing SQLite connection. Only the session's two threads send it jobs. | `core/src/store/mod.rs` |
| StoreReader | A read-only connection that the core's calls share. | `core/src/store/mod.rs` |
| Mirror worker | Every write to the Mirrors: clones, fetches of PR heads, the commits the user's Reviews covered, upkeep, and each Repository's Lookup checkout, which it moves to the PR on screen and tells readers where it is once it stops moving. Between git jobs it reads the per-file patch-ids the Stale review check compares, and hands them to the session to save, with the commits it can't read and why. It holds its queue as `fetching` says, and asks `mirror_queue` which job runs next: the user's demands first, a move of the Lookup checkout and then the heads of the PRs they opened, in a fetch of their own; then the rules' wants, each the whole set for a Repository, which replaces the last; then clones and upkeep. | `core/src/mirror.rs`, `core/src/mirror_queue.rs` |
| Highlighter | The parsed trees of the open PR's files, and of the few other files the Definition peek and the file view showed whole last. | `core/src/diff/highlighter.rs` |
| Settings owner | `settings.toml` and `machine.toml`. Readers get a `latest` copy. | `core/src/settings.rs` |
| Language servers | A server for each Repository and language: ty and clangd start when the PR on screen changes a Python or C file, and rust-analyzer on the first lookup in Rust. Each has the PR's files in its language open on it, and clangd the last 16 other files a lookup asked in too, as it answers only in an open file. Each lookup is a message, which waits for the Mirror worker's Lookup checkout and for the server to start, and never for the window. The thread sends the server the request and goes on: a thread of the lookup's own waits for the answer and hands it back as a message, and a newer lookup or a cancel cancels the one running. Between messages it stops each server no lookup has reached for the idle time, which `machine.toml` sets. A thread of each server's own watches it: it reads how far clangd's index has got from its `$/progress` and tells this thread, which tells a lookup that answered while clangd indexed, and asks clangd again for it once the index is built; it tells this thread when a server exits on its own; and from clangd's diagnostics, or a minute without a report of its index, it tells this thread of a problem clangd runs with. This thread keeps each server's row in Settings › Advanced, its state, last problem and version, and hands the rows to the window through a `latest` channel and an event. A path changed in Settings stops the server it replaces, and a thread of its own runs the new one with `--version`. | `core/src/definitions.rs` |
| Log | `tsugiki.log` and the tokens to scrub from it. | `core/src/logfile.rs` |
| Text search | What the latest query found, which the window reads through `latest`. Each query runs on threads of its own, up to 4, that send it what they find, and a newer query or the window's cancel cancels the one running. Each query names the ends of the diff it searches, and asks the Mirror worker where the Lookup checkout is, which the Files tab's read of that diff moved there: it searches nothing if the checkout is at another diff, and the window then reads the diff again once. It never moves the checkout, as two queries on their own threads would send their moves in either order. The rail's Search asks it queries, and asks for the state each time it says the state may have changed. | `core/src/search.rs` |
| Release check | Nothing shared. Asks GitHub Releases for a newer Tsugiki at start-up and every 6 hours, without the token, unless the settings turn it off. | `core/src/releases.rs` |

`fetching` is a handle with no thread. The core's calls and the session send each request for GitHub or the
Mirrors through it, and the two queues behind it keep their owners
([ADR 0022](adr/0022-the-users-demands-and-the-rules-wants-go-through-one-handle-to-two-queues.md)). The user
asks with demands, urgent, the newer winning; the rules in `work` state wants, the whole set for a Repository
each time. The GitHub thread does one request per step (`sync::Syncs`) and takes each demand between two
steps, so no demand waits for longer than one request
([ADR 0015](adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)). The session thread takes
the rest at once, so a mark, a change to the settings or the Mirror's patch-ids never wait on GitHub.
The window never waits on the session to read the database: the StoreReader reads while the Store saves
([ADR 0018](adr/0018-the-window-reads-the-database-on-its-own-read-only-connection.md)).

`app/src/wiring.rs` starts all of them in `connect_core` and hands the commands what they need through
Tauri's managed state: the `Calls` handle, which it makes from the session and starts the open Repository's
sync and the clones with, and beside it the settings owner, text search and the `latest` copies of the
app-wide status.

## How a screen gets its data

1. A screen in `ui/src/lib/state/` calls a command, such as `workbench`, through the generated
   `ui/src/lib/data/bindings.ts`.
2. The command, in `app/src/lib.rs` or `app/src/slow_commands.rs`, parses its arguments and makes one call
   on the core's handle, `Calls` in `core/src/calls.rs`: the screen's call, such as `Calls::workbench` or
   `Calls::pr_files`, or the user's action, such as `Calls::mark_seen`. That handle is all `crates/app`
   sees of how the core reads and fetches: `fetching`, the StoreReader and the view models' raw reads are
   the core's own, so a command can't run a query or send a demand itself. It sees the answers' types, which
   the commands return and the bindings name, and the events.
3. The view model, in `core/src/workbench/`, reads the stored facts in one transaction, has `work` judge
   them, and works out what the screen shows: **Needs attention**, Issue trees with their **Backports**,
   counts. Its `read` sends nothing.
4. The screen's call works out what the read lacks, queues through `fetching` the job that brings it, and
   answers. An answer that says its data is on the way holds that job
   ([ADR 0023](adr/0023-the-core-works-out-what-a-screens-read-lacks-and-a-pending-answer-holds-its-job.md)).
   The Files tab's read says what the Mirror lacks and which diff it shows. Its call queues the clone of a
   Repository the settings list, the PR's head first when the Mirror lacks it, or a read of the PR's
   Conversation when Tsugiki knows no head, and moves the Lookup checkout to the diff's ends, where text
   search looks for it. Opening a PR reads its Conversation each time. The store keeps one head per PR,
   the one named by the newer of the PR list's read and the Conversation's, by GitHub's `updatedAt`. A
   demand for a read already waiting in the GitHub thread's queue, or under way until GitHub answers, shares
   that read.
5. The GitHub thread announces a `Change` on the `StatusBoard` for the items a sync saved, and the session
   thread one for the items a mark or the Mirror's patch-ids changed. The app sends it to the window as the
   `changed` event, and each open screen that shows one of those items calls its command again. No event
   carries a view. While an Issue or a PR is on screen, the GitHub thread reads it from GitHub
   again a minute after its last read (`session/on_screen.rs`), and the window holds no timer: it tells the
   core when the user opens, hides, shows again or closes an item, and when the window loads, hides or
   shows. An item that shows again after its minute came round reads at once. The settings owner's thread
   hands the session each Repository's settings a change saves otherwise, with those they replaced, in the
   order it saves them. The session follows the move of Reach between the two in any Repository, opened or
   not. Those a hand edit changed in a Repository opened since launch, as new Conventions or Filters,
   announce a `Change` that names no item, and the Repository's lists read their items again. The session
   keeps no copy of the settings: each sync step, the rules' wants and each move of Reach read them from the
   owner's `latest` view, where the owner publishes a save and hands it over in one step under the view's
   lock, so the session reads settings at least as new as every save it has heard of. The view counts the
   saves handed over. A Mute or an Unmute reads the count with the settings, and first follows the saves
   counted that it hasn't, which are in its queue by then, so its move of Reach starts where theirs end.
   The messages it passes keep their order, and each hand edit it follows early announces its `Change`,
   opened or not, as a Sync it passed may open the Repository. A Repository the settings stop listing goes
   to the defaults, in the view and in a save.

App-wide status (the connection, the last sync, the hour's GitHub points, the request to GitHub in flight,
cloning progress, a Mirror's problem, each Mirror's state, whether the Mirror worker holds back git, a newer
version) takes a shorter path. The app keeps the newest value in a `latest` copy for its command, then emits
an event. A window that starts listening late asks the command and still gets the newest value.

A diff takes a third path, because it is too big to send whole
([ADR 0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)). The core runs `git diff` in the Mirror
and keeps the rows. The Files tab asks for the rows near what it shows, and for their highlighting from the
Highlighter.

Keys take a fourth path, its defaults fixed when the app is built. `core/src/keys.rs` lists each action the
user can bind, with its default key and where it works, and `npm run bindings` writes that table to
`ui/src/lib/data/keymap.ts`, a file apart from `bindings.ts`, which the UI's tests mock whole. The user's
remaps come with the settings: `core/src/keys.rs` reads each line of `[keys]` into a typed key, and the
window's settings hand them to `ui/src/lib/state/keymap.svelte.ts`, which lays them over the defaults.
Screens bind actions, and that module finds the action on a key.

## Where the data lives

- **What GitHub says**, and the user's own state such as Last seen, is in one SQLite file, `tsugiki.db`
  ([ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). The store keeps facts, not
  view models. `core/src/store/migrate.rs` moves an old file to the current schema.
- **Code** comes from each Repository's Mirror, a bare clone that Tsugiki owns. It doesn't read or write the
  user's own clones, apart from borrowing their objects when a Mirror is first cloned
  ([ADR 0002](adr/0002-tsugiki-owns-its-git-state.md)).
- **Settings** are TOML files: `settings.toml` travels between machines, and `machine.toml` doesn't
  ([ADR 0014](adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)).
- `core/src/folders.rs` says where all of these go, installed or portable.

## The core's modules

`core/tests/layers.rs` lists every top-level module, lowest first, with the modules each one may use and
why. Only `store` may hold a database connection: Clippy rejects `rusqlite::Connection` anywhere else.

The big ones, in the order data passes through them:

| Module | What it does |
|---|---|
| `github/` | The GitHub client: the GraphQL queries and their answers, parsed into the types of `facts`. It talks through a `Transport`, so tests replay recorded answers. It sums the points each answer cost this hour (`points.rs`), and keeps the rate limit's gate, which each request passes: below the floor of points, or after GitHub refused a request for its rate limit, it sends nothing until the reset and answers at once with the time the points come back (`gate.rs`). It tells the status board what each request asks as it goes, and when it ends, from what the caller says it asks (`in_flight.rs`). |
| `facts.rs`, `facts/` | The facts `work` judges: what GitHub says of each Issue and PR, the patch-ids the Mirror worker reads, and `WorkFacts`, the bundle a screen reads from the store. Neither the client nor the store owns them. |
| `sync/` | Decides what to ask GitHub for and how much at a time, and pauses while the client's gate holds: the first sync in tiers, then `refresh`, which ends by fetching the Issues the PRs it holds link to and the store lacks, asking the store again before each batch (#980), then reading ahead the Conversations `work` names, asking it again before each read. After each step that saves, it archives what `work` says closed (#820), from the closings the store kept as it saved, brings back to the Inbox each Archived Issue `work` says a new PR opened on (#823), from the openings the store kept, and saves its place. |
| `store/` | Saves and reads the facts, a file for each kind: Issues, PRs, activity, Filters, sync state. [schema.md](schema.md) draws its tables. It keeps the links each PR names under each Conventions, which `links` works out as the PR is saved, and the names of the rules that worked them out (`pr_links.rs`). It opens with links an older set of rules kept, and answers from them until the session refills them. It refuses to open on a rule's name the build doesn't know, as on a schema past its last migration. A `Snapshot` is one read transaction for a view that works out what to read next from what it read, as the Inbox, the Issue view and the PR view do. |
| `links.rs`, `conventions.rs` | Which Issues a PR fixes and which **Origin PR** a Backport comes from, read through the Repository's Conventions ([ADR 0011](adr/0011-pr-links-from-github-signals-by-repository-conventions.md)). Each link rule is a variant of `LinkRule` in `links/rules.rs`, and only the rules of `LinkRule::SET` run ([ADR 0024](adr/0024-the-link-rules-are-a-set-of-named-rules-parallel-to-the-schemas-migrations.md)). |
| `work.rs`, `work/` | The functional core: it takes `facts` and returns values. By Issue or PR number it judges what's on the **Workbench** in its own right (`reach.rs`), why it **Needs attention** (`causes.rs`) and each PR's review state (`review_state.rs`). By Repository it works out **Reach** from those and the links, and from Reach the heads the Mirror fetches, the commits it reads patch-ids of, the Conversations worth reading ahead, and the checks the store keeps. `sync` and `session` use it, never `workbench`. |
| `workbench/` | Groups what `work` judges into the Workbench's items, Issue trees with their PRs and **Backports**, and sums each item's Needs attention and its part in Mark Seen over its tree (`attention.rs`). Builds the view models: the Issue list, the Issue view, the PR view and its Files tab, quick open's search (`quick_open.rs`), and the **Inbox** a page at a time (`inbox.rs`), which walks the updates down from where the page starts and reads the trees of what it meets, so a page costs the same on a cache of any age. Those pages, the Issue view and the PR view read only the trees they show, through the links the store keeps and the store's `Snapshot` (`trees.rs`). The Workbench reads every Issue and PR, as it judges each one for Needs attention and the Sidebar's counts (#842). A read sends nothing, and `layers.rs` keeps it from `mirror` and `fetching`. |
| `fetching.rs` | The one entry point for requests for data: the user's demands and the rules' wants, handed to the session's queue, which passes those for GitHub to the GitHub thread, or to the Mirror worker's git queue ([ADR 0022](adr/0022-the-users-demands-and-the-rules-wants-go-through-one-handle-to-two-queues.md)). |
| `calls.rs` | The handle `crates/app` reads and fetches through, `Calls`, made from the session, with a call for each screen and a method for each of the user's actions. A screen's call reads through a view model. One whose read can find data missing, as the Files tab's can, queues through `fetching` the job for what the read lacks, and answers. An answer that waits for data holds the job's `Queued`, which only `fetching` makes ([ADR 0023](adr/0023-the-core-works-out-what-a-screens-read-lacks-and-a-pending-answer-holds-its-job.md)). Each action goes through `fetching` or the settings owner. |
| `patch_ids.rs` | Per-file patch-ids, and which files changed between the reviewed commit and the head, for the Stale review check. |
| `mirror.rs`, `mirror/` | The Mirror worker's thread and queue, and the git it runs: cloning, fetching PR heads, keeping the commits the user's Reviews covered, reading patch-ids, upkeep, moving the Lookup checkout (`mirror/lookup.rs`). It keeps apart, for each PR, where the mirror's ref stands, the head the rules want and the head the user opened (`mirror/heads.rs`). It runs each job in the order `mirror_queue` picks. |
| `mirror_queue.rs` | In the functional core: picks the Mirror worker's next job from what it holds queued in each Repository, what it found of git and the disk, and whether GitHub can be reached. Its tests need no thread and no git. |
| `mirror_source.rs` | Where each Repository's mirror gets its objects, which `settings` keeps and `mirror` clones by. |
| `diff.rs`, `diff/` | A PR's diff from its Mirror, its per-file patch-ids, NEWS entries, find in the diff, and the Highlighter. |
| `definitions.rs`, `definitions/` | Where a name is defined, which it asks a language server through `tsugiki-lsp`, in the Lookup checkout ([ADR 0020](adr/0020-go-to-definition-asks-a-language-server-which-reads-a-lookup-checkout.md)). `definitions/server.rs` runs one server on the checkout and writes its log, `definitions/health.rs` holds each server's state, last problem and version, and checks a path the user sets, and each server has a module of its own for how it starts: `definitions/ty.rs` for Python, `definitions/rust_analyzer.rs` for Rust and `definitions/clangd.rs` for C, which writes clangd's compile database in a Repository with CPython's layout. |
| `highlight.rs`, `highlight/` | tree-sitter highlighting, one table row per language ([docs/highlighting.md](highlighting.md)). |
| `search.rs`, `search/` | Text search over the open PR's code at its head, on ripgrep's crates, in the Lookup checkout and the Mirror (ADR 0020). |
| `session.rs`, `session/` | The session thread, which takes the messages `fetching` sends it and passes those for GitHub to the GitHub thread (`session/github_thread.rs`). After each change to the settings, each Mute and each Unmute, and each save of the GitHub thread's that can change them, it hands `fetching` the rules' wants of the Mirror. When the settings or an Unmute put PRs in Reach, it has the store owe a read of each Conversation the rules say they owe. |
| `status.rs` | App-wide status and the `StatusBoard` that announces it. |

The smaller ones say what they do in their first `//!` line.

## Where tests draw the line

- `token/src/live.rs` holds everything that touches the real network, `gh` and `GH_TOKEN`, and the core's
  `github/live.rs` runs the real `git` and hands requests to the token crate's client. Tests swap the
  network for the replayer in `github/replay.rs` and `gh` and `GH_TOKEN` for stand-ins in
  `token/src/testing.rs`, and `github/record.rs` captures new answers
  ([ADR 0009](adr/0009-testing-and-full-coverage.md)).
- `app/src/wiring.rs` sets up the commands and events on any Tauri runtime, so its tests run on Tauri's
  mock runtime without a window. `app/src/startup.rs` opens the real window, and only `just smoke` tests it.
  What it decides goes in a module its tests can reach, such as `app/src/first_frame.rs`, which paints the
  window before it shows.
- The UI's state files have Vitest tests beside them, with the bindings mocked. `ui/e2e/` runs the built UI
  in Chromium and WebKit with Playwright, with Tauri's IPC mocked in `ui/e2e/mock-core.ts`.
- `just stale-bindings` fails when `bindings.ts` no longer matches the Rust commands, or `keymap.ts` the
  core's key table. `just bindings` writes both again.
