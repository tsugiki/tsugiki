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
  the head, and the store keeps that head in the PR's facts when they have none. Every reader of the facts
  then sees it: the Files tab, the Mirror's head wants, Reach, Stale review and patch-ids.
- A job that brought nothing new ends the wait. When the session's read of a Conversation since launch
  named no head, the Files tab says so, where queuing the read again would bring the same, until a demand
  asks for another read. A Conversation an older Tsugiki saved kept no head, so its PR's tab reads the
  Conversation again.
- A demand for a read already waiting in the session's queue, or under way until GitHub answers, shares
  that read, whose outcome, a failure too, answers both. A demand that comes after GitHub answered starts
  another. Opening a PR and its Files tab's read then ask GitHub once between them.
- The Files tab queues the clone only of a Repository the settings list. Another has no mirror to wait for,
  and its read says so.

## Considered Options

- **The answer types in `workbench`, the token in a module below it.** Only the module that defines a private
  field can build its type, and `workbench` may not use `fetching`, so any module could build such a token.
  The answers live in `calls`, which may use both.
- **A `Queued` made by any send on a channel.** A send nobody reads would make one too.
- **The demands stay in the command.** Each command decides what to fetch its own way, as #843 found of the
  settings that three commands pushed into the session.
- **The Files tab's read takes the head from the Conversation.** Only the Files tab would see it. The
  Mirror's head wants, Reach and Stale review would go on without it.
- **Each demand carries the instant it was asked, and a read that finished after it answers it.** That
  guesses from the order of two instants. A demand that came during a read that failed counted as answered,
  and nothing asked again.
- **A migration that copies each kept Conversation's head into the PR's facts.** A Conversation read long
  ago can name a head the PR has moved on from, and the Mirror would fetch that first. Opening the PR reads
  its Conversation again, and that read fills the facts.

## Consequences

- A test of a call checks the answer, and what reached a session and a Mirror worker that only listen.
- Each read of a Files tab that waits for its Mirror queues the clone again. The Mirror worker leaves a clone
  under way, waiting or failed as it is.
- The Mirror worker still awaits the head of a PR opened before Tsugiki knew it. After each Conversation
  the session hands it the head the PR's facts keep, the one the Files tab asks for, as it hands a sync
  step's, so the head keeps the demand's rank whichever names it first. The session merges a read's
  demands before it hands the head over, so the worker hears each await they sent before the head that
  answers it. A head the store filled, or a Review of the user's that covers another commit, sends the
  rules' wants again.
- A Conversation that names no head, or a PR the facts don't hold, ends the await. A read that fails leaves
  it in place until a sync step names the head.
- The session's queue keeps the messages it took off the channel while merging, ahead of the channel, in
  the order they came.
- `Fetching` and the session share the PRs whose Conversation named no head in one list behind a lock. The
  session adds a PR, and a demand for the PR's Conversation takes it off.
- `crates/app` can still name `Fetching` and a view model's raw `read`, so nothing yet stops a command from
  sending a demand itself.
