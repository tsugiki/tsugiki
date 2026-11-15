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
- A rail on the right of the PR view sits beside the Conversation and the Files tab
  ([#603](https://github.com/tsugiki/tsugiki/issues/603)). It has tabs
  ([#727](https://github.com/tsugiki/tsugiki/issues/727)): Files, for the tree of the PR's changed files
  ([#604](https://github.com/tsugiki/tsugiki/issues/604)), and Search, with Review to come between them.
  Each tab has a button at the right of the PR's tab row, a key and a palette command, which open the rail
  on that tab: the Files button, `Ctrl+K F` and "View: Files in the Rail". The button or key of the tab on
  show closes the rail, and so does × in its corner, though Search's key focuses its box instead. It stays
  open, at its width and on its tab, from one of the PR's tabs to the other and from PR to PR, until the
  user closes it or Tsugiki quits.
  - A row of tabs at the top of the rail shows once it has two tabs or more. A click on a tab shows it, and
    a click on the tab on show closes the rail, though Enter or Space on it leaves the rail open, as the ARIA
    tabs pattern has them confirm the tab. ← → move between the tabs while the row holds the focus, and Home
    and End go to the first and the last.
    The rail opens on the tab the user picked last, for the session. A tab can put a count in its label,
    as "Review · 3", on its button too, and keeps it while another tab shows.
  - Its left edge drags its width, from 200 to 640 px, while the tab beside it keeps 240; a double-click
    puts back its 280. The pane layout keeps the width in `machine.toml` ([layout.md](layout.md)). Beside
    the rail the Conversation scrolls by itself under the tab row, as the diff does.
  - Esc in the rail gives the focus to the diff, or the file view over it, or the Conversation, and leaves
    the rail open. Closing the rail with the focus in it does the same, and from the Conversation, which the
    whole pane scrolls again, gives the focus to the button of the rail's tab that showed.
  - Search ([#686](https://github.com/tsugiki/tsugiki/issues/686)) searches the PR's code at the head of
    the diff the Files tab shows, as the core's text search does ([definitions.md](definitions.md)).
    `Ctrl+Shift+F` and "View: Search in the Rail" open the rail on it with the focus in its query box,
    filled with the text selected on one line, or else the word picked in the diff or the file view.
    Pressed again with Search on show, the key focuses the box, and puts in it only text selected on one
    line outside it, as in VS Code: the query the user typed stays otherwise. The box has three toggles,
    match case, whole word and regex, on `Alt+C`, `Alt+W` and `Alt+R` while it has the focus, and it
    searches 250 ms after the last key. The tab reads the PR's diff on either of the PR's tabs, as each
    query names the diff's base and head, and searches the checkout that reading moved there. It searches
    again once a diff with another base or head shows, as a PR moved to another Target branch gets
    ([#211](https://github.com/tsugiki/tsugiki/issues/211)). A query nobody wants any more, as the box
    empties or the PR closes, stops.
  - A line under the box says "312 results in 41 files", "Stopped at 5,000 results", "Waiting for the
    checkout…", why the diff the search needs isn't there, or the regex's error. A match opens only in
    the diff it was found in. The results group by file: a row with the path, the count and a fold
    arrow, then a row for each line with its number and text, each match marked. A Generated file's rows
    are greyed and say so, as in the Files tab. The list draws only the rows on screen.
  - A click or Enter on a match in a file the PR changes shows the Files tab at that line, as Go to
    definition lands, opening the lines around it if no hunk shows it. In any other file it opens the file
    view at the line, as Go to definition does, or lands on the line in the file view on show if it shows
    that file. ↑ ↓ move through the rows, ← → fold and unfold a file, and Enter opens. The query, its
    toggles and what it found stay while the PR's view lasts, so closing and reopening the rail keeps
    them.
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
  thread**. Go to definition opens a hidden line the same way ([definitions.md](definitions.md)).
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
