# The core works out what a screen's read lacks, and a pending answer holds its job

[ADR 0022](0022-the-users-demands-and-the-rules-wants-go-through-one-handle-to-two-queues.md) kept a view
model's `read` pure, and gave the Tauri command what came after it: `pr_files` read the Files tab, then asked
`fetching` for the PR's head, the opened PR's ref and the Lookup checkout's move. Each command decided what to
fetch. For a PR with no head the command called `await_head`, which waits for a sync step to name the head. A
sync never reads a merged PR again, so its Files tab showed Fetching for good (#782).

This supersedes the part of ADR 0022's "Reads send nothing" that gives the command the demands. A view model's
`read` still sends nothing, and the read path of
[ADR 0018](0018-the-window-reads-the-database-on-its-own-read-only-connection.md) stays, behind the core's
call.

## Decision

A screen whose read can find data missing makes one call into the core, in `calls`. The call reads through
the view model, queues through `fetching` a job for what the read lacks, and answers. The command parses its
arguments and makes the call.

- The read says what the Mirror lacks in its own type. `FilesRead` names no Mirror, no head, a head the
  Mirror lacks with the older diff it holds, or the diff at the head. `HeadInMirror::of` says it for opening a
  PR. Neither queues anything, so their tests need no worker.
- An answer that waits for data holds a `Queued`, which only `fetching` makes, as it queues the job: the Files
  tab's Cloning and Fetching, and the threads it holds back while a newer head comes. A pending answer can't
  be built while nothing brings its data, and a wait for a step nobody scheduled, as `await_head` is, makes
  no `Queued`.
- The job has to bring what the read looks at, or the read finds the same lack after the job saves and
  queues it again on each `changed`. A PR whose head Tsugiki doesn't know reads its Conversation, which names
  the head, so the read takes a PR's head from its Conversation when the sync has none.

## Considered Options

- **The answer types in `workbench`, the token in a module below it.** Only the module that defines a private
  field can build its type, and `workbench` may not use `fetching`, so any module could build such a token.
  The answers live in `calls`, which may use both.
- **A `Queued` made by any send on a channel.** A send nobody reads would make one too.
- **The demands stay in the command.** Each command decides what to fetch its own way, as #843 found of the
  settings that three commands pushed into the session.

## Consequences

- A test of a call checks the answer, and what reached a session and a Mirror worker that only listen.
- Each read of a Files tab that waits for its Mirror queues the clone again. The Mirror worker leaves a clone
  under way, waiting or failed as it is.
- The Mirror worker still awaits the head of a PR opened before Tsugiki knew it, so a sync step that names
  the head before the Conversation comes keeps the demand's rank.
- `crates/app` can still name `Fetching` and a view model's raw `read`, so nothing yet stops a command from
  sending a demand itself.
