# In-app review

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The whole PR scrolls as one list, in unified or side-by-side view.
- The Files tab has two modes, all files and one file at a time
  ([#605](https://github.com/tsugiki/tsugiki/issues/605)). All files, the default, is the one list above
  ([ADR 0001](../adr/0001-standalone-tauri-gui-with-in-app-review.md)). One file at a time shows a single
  file under a bar that reads "‹ 12 of 48 ›" and the file's path. `settings.toml` picks the mode every PR
  opens in:

  ```toml
  [diff]
  files = "one"   # "all" or "one"
  ```

  A value other than the two keeps "all", and the status bar names the file and the line. The toggle in
  the Files tab's header, `Ctrl+K O` and "View: One File at a Time" / "View: All Files" in the command
  palette switch the PR on show until it closes. Switching keeps the place: the file at the top of the tab
  becomes the one shown, and switching back scrolls to where it was. Tsugiki never switches by itself,
  however large the PR.
- One file at a time, the NEWS entries come first, on a page of their own, then each file in the order of
  all files, a Generated file collapsed to its header as there. The threads on files no longer in the diff
  get a last page. `Alt+PageDown` and `Alt+PageUp` go to the next and the previous page, and stop at either
  end; with all files they scroll to the next or the previous file's header. `F7`, `F8` and Find in diff
  cover the whole PR, and show the file of the change, the thread or the match they move to. So does
  anything that opens a file: a thread opened from the Conversation, and a NEWS entry's Show file. After a
  move to a page, `F7` and `F8` go on from there.
- Hidden lines around each change can be shown, as on GitHub: 20 more above or below a hunk, the rest of a
  short gap, or the whole file. They are read from the file at the PR’s head in the mirror, so expanding costs
  no API call. New line comments stay on lines of the diff, because GitHub's API accepts them only there; the
  comment-anywhere of github.com's Files changed page has no public API
  ([research](https://github.com/tsugiki/tsugiki/blob/research/expanded-line-comments/research/expanded-line-comments.md)).
  Expanded lines show no + in the gutter, and hovering it says why: reply to a thread there, or comment on the
  PR.
- Each file's header has a ▸/▾ chevron that collapses the file to its header or opens it, as on github.com.
  Generated files start collapsed and shown with line counts, and the rest start open. Whether a file is
  generated comes from `.gitattributes` on the PR's **Target branch**. Tsugiki saves nothing of a click and
  sends nothing to GitHub, so a reopened PR starts each file as before. A jump into a collapsed file
  (`F7`, `F8`, Find in diff, **Go to definition**) opens it, and the rows of a collapsed file aren't
  highlighted until it opens.
- A PR whose user's Review is a **Stale review** still opens on the Conversation tab
  ([#155](https://github.com/tsugiki/tsugiki/issues/155)). Both tabs show one line under the tab bar: "Your
  Review at abc1234 is stale: 3 files changed, 1 only in whitespace", with the commit the Review covered,
  and a Review changes button that opens Files at the first changed file
  ([#387](https://github.com/tsugiki/tsugiki/issues/387)). In Files, a changed file's header says "Changed
  since your Review" or "Whitespace only", and each file the Review left unchanged starts collapsed, as a
  Generated file does. `Ctrl+K J` and "View: Unfold Unchanged Files" in the command palette open them all.
  The diff stays the whole PR; the interdiff since the Review is for later.
  - Files marks and collapses files only when the check read the head the diff shows. While a newer head
    waits for its check, the PR keeps the last verdict, of an older head, which could call a file unchanged
    that the newest push changed. Until the check catches up, Files collapses only Generated files, and
    Review changes opens it at the top.
  - A Stale review whose check can't run has no files to name, so the line gives the reason in place of
    the counts ("Your Review at abc1234 may be stale: Tsugiki can't check it until the mirror is cloned"),
    with no Review changes, and Files collapses only Generated files.
  - A **Bot backport** shares its Origin PR's review state, so its line names the Origin PR, "Your Review
    of #123 at abc1234 is stale", and counts the Origin PR's files. That check read the Origin PR's
    commits, not the Backport's, so the line has no Review changes, and the Backport's Files marks
    nothing and collapses only Generated files.
- A PR whose commits the mirror doesn't hold yet says "Fetching the PR's commits…" until they arrive. When
  the PR has moved on since the mirror's copy, the older diff stays readable under "Showing abc1234 · fetching
  the newer def5678", with the threads held back and counted, since their lines belong to the newer commit.
  The newer diff then takes its place at the same file. A fetch that fails says so, keeps the older diff and
  offers Try again.
- Offline, the mirror holds back git, so the Files tab shows what the mirror holds and says what waits for
  the network: "The PR's commits come once GitHub answers.", or "Showing abc1234 · the newer def5678 comes
  once GitHub answers." over the older diff. Before a Repository's mirror is cloned, it says "Cloning
  python/cpython starts once GitHub answers." Back online, the fetch or the clone starts by itself, and the
  diff follows without a click. A PR whose head the mirror holds shows its diff offline as it does online,
  with its highlighting, expanded lines and threads.
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
