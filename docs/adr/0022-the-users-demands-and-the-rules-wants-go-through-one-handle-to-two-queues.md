# The user's demands and the rules' wants go through one handle to two queues

Before this, the core took requests for data in about six places. The app and the session called the
session's requests, `app/wiring.rs` forwarded the session's Mirror work to the Mirror worker, and the Files
tab's view model moved the Lookup checkout and queued fetches from inside its `read`. Nothing ranked what the
window asked for against background work in one place, and a view model's read had side effects. The rules
also sat in the view models, so a change to how the list groups its items could change what Tsugiki fetches.

The core now has three layers, and follows functional core, imperative shell:

- **The rules**, in `work`, take facts and return values. By Issue or PR number they judge what's on the
  Workbench in its own right, why it Needs attention and the user's review state. By Repository they work
  out Reach, and from Reach the heads the Mirror fetches, the commits it keeps and reads patch-ids of, the
  Conversations worth reading ahead, and the checks the store keeps.
- **Grouping**, in `workbench`, builds the Workbench's items from the links and sums each item's Needs
  attention and Seen over its tree.
- **The screens**, in `workbench` too, lay the items out.

Only the rules decide what Tsugiki fetches or stores, so a change to grouping or to a screen can't change
either. `layers.rs` holds this: `facts`, `links`, `activity` and `work` may use no module of the shell, and
`workbench` may use none of `mirror`, `fetching`, `session` and `sync`.

## Two actors ask for data

- **The user** asks through the window, with **demands**: "PR 801 opened", "the Files tab shows these ends".
  A demand is urgent, and where two conflict the newer wins, as when the Lookup checkout follows the PR on
  screen.
- **The rules** state **wants**: every head that should be in the Mirror, every commit the user's Reviews
  covered, every commit whose patch-ids the Stale review check needs. A want is the whole set for a
  Repository and replaces the last one, and the Mirror worker's queue works toward it. A head that leaves the
  set before its turn isn't fetched. New facts start the rules again: the session works out the wants after
  each sync step that saves. The rules also name the Conversations worth reading ahead, which no queue
  holds: a refresh asks for them before each one it reads.

## One handle, two queues

`fetching` takes both. `Fetching` is a handle with no thread of its own, above `mirror` and below `sync` and
`session`, and the app and the session each hold a clone. It holds the Mirror worker's requests and the
sending end of the session's queue, whose messages it defines.

Two queues stay behind it, each with one owner. The session runs GitHub's, one request at a time
([ADR 0015](0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)), and the Mirror worker runs
git's. A GitHub read and a `git fetch` compete for nothing, so neither waits on the other. Each owner puts
the user's demands first, and what the rules name after them.

- The Mirror worker moves the Lookup checkout first, as a reader may wait for it. Then it fetches the heads
  of the PRs the user opened, in a fetch of their own, ahead of the heads the rules want, and of other
  Repositories' heads. Then the rules' heads, reviewed commits and patch-ids, then clones, then upkeep.
- The session takes each demand between two sync steps, so a demand waits for one request at most and goes
  ahead of the sync's tiers. The Conversations come last, after a refresh's own polling: before each read
  the refresh asks the rules for the set, from the facts and the reads the store owes, so the set is as new
  as the store. It asks GitHub for each PR once, which is the queue's bookkeeping, not a rule. A store that
  can't answer fails the step, which is retried like any other, so a refresh doesn't finish, and forget
  what the store owed, on a set it couldn't work out. When a switch of a Filter puts PRs in Reach, the
  rules also say which of their Conversations the store owes a read: those it doesn't hold read at the PR's
  head.
- The writes to GitHub that #393 starts will go ahead of all three.

A demand keeps its rank once GitHub names the head. When the user opens a PR before the session knows its
head, the core tells the Mirror worker to await it. Once a sync step saves heads, the session hands them over
too, and the worker puts the opened PR's head at the front of its queue, whether or not the PR is in Reach.
A want that leaves out an opened PR doesn't drop its head, or the fetch once more that a head GitHub hadn't
moved to yet gets.

## Reads send nothing

[ADR 0023](0023-the-core-works-out-what-a-screens-read-lacks-and-a-pending-answer-holds-its-job.md)
supersedes this section where it gives the command the demands: the screen's call in the core sends them.

Reads keep their paths: the StoreReader
([ADR 0018](0018-the-window-reads-the-database-on-its-own-read-only-connection.md)), `latest` copies, and the
diff rows. A view model's `read` sends nothing. The Files tab's read returns what it shows, with the ends of
its diff, and what the Mirror holds of the PR's head, which a pure function, `HeadInMirror::of`, works out
from what the read found in the Mirror. The `pr_files` command then sends the demands: the PR opened, its
head first when it's missing or once it's known, and the Lookup checkout to the ends the read showed. Text
search checks those same ends, so it finds the checkout there. Opening a PR does the same through the same
check, without reading a diff.

## Considered Options

- **A coordinator thread** between the window, the session and the Mirror worker, which would rank all
  requests. It would be a third party holding a copy of both queues, which drifts from the owners' copies,
  and each request would cross one more thread. The handle gives the same single entry point with no state
  of its own.
- **One queue for GitHub and git.** A fetch would then wait on a GitHub read and the other way round, though
  they share nothing.
- **Wants as additions,** as heads were handed over before. A head that left Reach stayed queued, and a
  Repository's queue only grew until a fetch emptied it.
- **Side effects in the view model's read,** as before. Each read of the Files tab moved the checkout, so a
  test of the read needed a fake Mirror worker, and a reader of the code found a request hidden in a read.

## Consequences

- Each request for GitHub or the Mirror from the app or the session goes through `Fetching`. The language
  servers' thread and text search still ask the Mirror worker where the Lookup checkout is, which is a read.
- The session's own thread holds a handle, so its queue never closes by itself: dropping `Session` sends it
  a stop.
- A want replaces the last only in what it states. When the store can't read the reviewed commits, the want
  leaves them out, and the Mirror worker goes on with the last ones.
- A test of a view model checks the values its read returns, and a test of the session checks what reached
  a Mirror worker that only listens.
