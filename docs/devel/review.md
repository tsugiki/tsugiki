# In-app review

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The whole PR scrolls as one list, in unified or side-by-side view.
- Hidden lines around each change can be shown, as on GitHub: 20 more above or below a hunk, the rest of a
  short gap, or the whole file. They are read from the file at the PR’s head in the mirror, so expanding costs
  no API call. New line comments stay on lines of the diff, because GitHub's API accepts them only there; the
  comment-anywhere of github.com's Files changed page has no public API
  ([research](https://github.com/tsugiki/tsugiki/blob/research/expanded-line-comments/research/expanded-line-comments.md)).
  Expanded lines show no + in the gutter, and hovering it says why: reply to a thread there, or comment on the
  PR.
- Generated files are collapsed by default and shown with line counts. Whether a file is generated comes from
  `.gitattributes` on the PR's **Target branch**.
- A PR whose commits the mirror doesn't hold yet says "Fetching the PR's commits…" until they arrive. When
  the PR has moved on since the mirror's copy, the older diff stays readable under "Showing abc1234 · fetching
  the newer def5678", with the threads held back and counted, since their lines belong to the newer commit.
  The newer diff then takes its place at the same file. A fetch that fails says so, keeps the older diff and
  offers Try again.
- Every **NEWS entry** in the PR is rendered at the top. A PR can have several, or none.
- A thread someone started on github.com on a line outside every hunk is shown in place: the hidden gap around
  it opens by itself to show that line with 3 lines either side, marked as outside the diff, and the thread
  sits under it like any other, placed by its file, line and side. It can be replied to and resolved like any
  thread, and `F8` reaches it in order. If its line no longer exists at the head, it is an **Outdated
  thread**.
- **Outdated threads** are grouped at the top of their file together with the code they were written against:
  the last 4 lines of the hunk GitHub stored with the thread, ending at the commented line, and the whole hunk
  on "show more". During a re-review, the user's own unresolved Outdated threads are listed first.
- A thread on a whole file sits at the top of that file, above its Outdated threads, marked "on the file".
- A thread is matched to a file by its path, as the file's name or its name before a rename. A thread that
  matches no file in the diff, because a later commit took the file's change back out, goes in "On files no
  longer in the diff" at the end of the Files tab, shown like an Outdated thread.
- A resolved thread folds to one line under its code: the first commenter, the number of comments and
  "Resolved". `Enter` or a click opens it. `F8` and `Shift+F8` go through the unresolved threads: in each file
  the threads on the file first, then the Outdated ones, then those in the diff, and last the threads on files
  no longer in the diff.
- A comment hidden on GitHub (as off-topic, outdated, resolved, a duplicate, spam or abuse) folds to one line
  with its reason, as on GitHub, and opens on a click. A thread whose comments are all hidden counts as
  resolved.
- The Conversation tab shows each **Review** as one block: its author, state, commit and text, then each of
  its threads as one line (file and line, the first words, the replies, Resolved or Outdated) that opens the
  thread in the Files tab. A Review with no text and no threads is one line, such as "some-reviewer approved
  at abc1234". A thread itself shows only in the Files tab.
