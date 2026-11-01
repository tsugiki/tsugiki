# Line comments on expanded context lines

Research for [tsugiki/tsugiki#5](https://github.com/tsugiki/tsugiki/issues/5). Checked 2026-11-01.

## Question

The diff view can expand hidden lines around a hunk (read from the file at the PR's head). Can a review comment be anchored to one of those lines through GitHub's public API, as a single line or a range, on either side? If so, with which parameters?

## Answer

No, not through the public API. Both REST and GraphQL accept a line comment only on a line that is inside one of the diff's hunks: a changed line or one of the (usually three) context lines GitHub puts around each change. A line that is only visible after expanding is refused:

- REST (`POST /repos/{owner}/{repo}/pulls/{pull_number}/comments` and the `comments[]` of `POST .../pulls/{pull_number}/reviews`) returns **422 Validation Failed** with `Pull request review thread line must be part of the diff`. A range whose start is not in the same hunk as its end also gets `Pull request review thread start line must be part of the same hunk as the line.`
- GraphQL `addPullRequestReviewThread` returns no error but `thread: null`, and nothing is created.

The github.com web UI is different. Since September 2025 its new "Files changed" page lets a reviewer comment on any line of a changed file, expanded lines included. Those comments are real review comments and come back from the REST and GraphQL read APIs, but GitHub has said that API support for creating them is "limited", and as of October 2026 users still report that the API rejects them.

So Tsugiki should keep refusing line comments on expanded lines, but it must be able to **show** such comments made by others on github.com (see "What this means for Tsugiki").

## What the docs say

### REST: create a review comment

Source: [Create a review comment for a pull request](https://docs.github.com/en/rest/pulls/comments?apiVersion=2022-11-28#create-a-review-comment-for-a-pull-request); same text in the OpenAPI description at [github/rest-api-description](https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json), fetched 2026-11-01.

- Endpoint: "Creates a review comment on the diff of a specified pull request."
- `line`: "**Required unless using `subject_type:file`**. The line of the blob in the pull request diff that the comment applies to. For a multi-line comment, the last line of the range that your comment applies to."
- `side`: "Use `LEFT` for deletions that appear in red. Use `RIGHT` for additions that appear in green or unchanged lines that appear in white and are shown for context. For a multi-line comment, side represents whether the last line of the comment range is a deletion or addition."
- `start_line`: "The `start_line` is the first line in the pull request diff that your multi-line comment applies to."
- `subject_type`: "The level at which the comment is targeted." Values `line`, `file`.
- `position` (deprecated since 2022-11-01): "The position value equals the number of lines down from the first "@@" hunk header in the file".
- Errors: 422 "Validation failed, or the endpoint has been spammed." The docs do not list the validation rules.

The wording is "the line of the blob **in the pull request diff**". The docs never say that a line outside the hunks is accepted. "Unchanged lines that appear in white and are shown for context" describes the context lines inside a hunk, but it does not say whether lines revealed by expanding count too.

### REST: create a review

Source: [Create a review for a pull request](https://docs.github.com/en/rest/pulls/reviews?apiVersion=2022-11-28#create-a-review-for-a-pull-request).

- Note on the endpoint: "To comment on a specific line in a file, you need to first determine the position of that line in the diff."
- `comments[]` takes `path`, `position`, `body`, `line`, `side`, `start_line`, `start_side`. Only `path`, `position` and `body` have descriptions; `line`/`side`/`start_line`/`start_side` have only examples (`28`, `RIGHT`, `26`, `LEFT`). Nothing more on what lines are allowed.

### GraphQL

Source: the public schema, [schema.docs.graphql](https://docs.github.com/public/fpt/schema.docs.graphql) (fetched 2026-11-01), and the [GraphQL reference for pulls](https://docs.github.com/en/graphql/reference/pulls).

- `addPullRequestReviewThread`: "Adds a new thread to a pending Pull Request Review."
- `AddPullRequestReviewThreadInput.line`: "The line of the blob to which the thread refers, required for line-level threads. The end of the line range for multi-line comments."
- `side: DiffSide = RIGHT`: "The side of the diff on which the line resides."
- `startLine`, `startSide: DiffSide = RIGHT`; `subjectType: PullRequestReviewThreadSubjectType = LINE` ("can be a diff line or a file").
- `addPullRequestReviewComment` is deprecated: every input field carries "**Upcoming Change on 2023-10-01 UTC** ... use addPullRequestReviewThread or addPullRequestReviewThreadReply instead". It only takes `position`, so it can only target diff positions anyway.
- `DraftPullRequestReviewThread` (the `threads` of `addPullRequestReview`) has the same `line`/`side`/`startLine`/`startSide` as above.

The schema does not say what happens for a line outside the diff.

### The web UI and the changelog

- [Pull request "Files changed" public preview now supports commenting on unchanged lines](https://github.blog/changelog/2025-09-25-pull-request-files-changed-public-preview-now-supports-commenting-on-unchanged-lines/) (GitHub changelog, 2025-09-25). The same text is in GitHub's feedback discussion [community#174786](https://github.com/orgs/community/discussions/174786):
  - "Using the new "Files changed" page, you can now comment on any line of a changed file. Previously, you could only comment on the three lines surrounding a change."
  - Steps: select a changed file, "expand all or part of the diff to reveal unchanged lines", click the `+`.
  - "These comments can only be added to files already changed (i.e., not unchanged files)."
  - "API support is currently limited, but comments on unchanged lines will be returned by existing APIs and in webhook events."
  - "If using the classic "Files changed" page to view a pull request with comments on unchanged lines, a warning message will appear explaining that there are additional comments only available on the new page."
  - It was public preview, "rolling out gradually on a per-repository basis". The author (who posted the official announcement) wrote on 2025-10-09: "The feature has been rolled out everywhere. You do need to "switch to the new experience" ... to add or view these comments." ([comment](https://github.com/community/community/discussions/174786#discussioncomment-14639726)). I did not find a later post saying the new page left preview.
- [GitHub Mobile: Comment on unchanged lines in pull request files](https://github.blog/changelog/2026-02-03-github-mobile-comment-on-unchanged-lines-in-pull-request-files) (2026-02-03): the same ability came to the iOS and Android apps.
- Before that, the classic page did not allow it: [community#141466](https://github.com/orgs/community/discussions/141466) (2024-10-14) asks for comments on lines revealed with "Expand Up/Expand Down", and its author later linked the two changelog posts as the answer.
- [Commenting on a pull request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/commenting-on-a-pull-request) (help docs) does not say which lines take comments.

## Evidence of what the API actually does

None of this is in GitHub's reference docs; it comes from error reports and from GitHub's own MCP server.

- **REST, out-of-diff line.** [community#187218](https://github.com/orgs/community/discussions/187218) "PR Review API - Post comment anywhere in changed files" (2026-02-15): both the create-review-comment and create-review endpoints "reject comments targeting lines that fall outside the @@ hunk ranges in the PR diff", with `422 Validation Failed "pull_request_review_thread.line" is not part of the diff`, while the web UI creates such comments on the same PRs. No GitHub reply; two more users confirmed in October 2026 (2026-10-16, 2026-10-17). An earlier ask in the feedback thread got no answer either ([comment](https://github.com/community/community/discussions/174786#discussioncomment-15410542), 2026-01-05).
- **REST, exact messages.** [parkerbxyz/suggest-changes#28](https://github.com/parkerbxyz/suggest-changes/issues/28) (2024-04) shows a full 422 response from `POST /repos/boozook/playdate/pulls/294/reviews`: `"Pull request review thread line must be part of the diff, Pull request review thread start line must be part of the same hunk as the line., and Pull request review thread diff hunk can't be blank"`. [ActionsDesk/github-actions-details-action#4](https://github.com/ActionsDesk/github-actions-details-action/issues/4) (2021) shows the first message on its own.
- **GraphQL, out-of-diff line.** [github/github-mcp-server#1214](https://github.com/github/github-mcp-server/issues/1214) (2025-10-14): `addPullRequestReviewThread` with `line`/`side` on an unchanged line "is silently failed". The fix, [github/github-mcp-server#1441](https://github.com/github/github-mcp-server/pull/1441) (merged 2025-11-19): "When line is out of range GraphQL API doesn't return error ... The indication that something like this happened is empty thread id." It now reports "The line number doesn't exist in the pull request diff". [github/github-mcp-server#2495](https://github.com/github/github-mcp-server/issues/2495) (2026-05-18, open) asks for comments "on any file line, not just lines within the diff" and confirms they are still dropped.

## What such comments look like when read (python/cpython)

Read-only `GET /repos/python/cpython/pulls/comments` for comments updated since 2026-09-15 (2,510 comments, 1,475 of them top-level line comments). 25 top-level line comments have an empty `diff_hunk`. I checked two against the diff at their commit:

- [python/cpython#150675 r4371972974](https://github.com/python/cpython/pull/150675#discussion_r4371972974), `Modules/itertoolsmodule.c`, `line: 3949`, `side: RIGHT`, commit `90803c8`. The file's RIGHT-side hunks at that commit cover 3917–3923 and 3962–3982, so 3949 is outside every hunk.
- [python/cpython#159721 r4317865294](https://github.com/python/cpython/pull/159721#discussion_r4317865294), `Modules/_zoneinfo.c`, range `start_line: 1422`, `line: 1425`, both `RIGHT`, commit `2247f61`. The only hunk is `@@ -1436,7 +1436,7 @@`, so the whole range is outside.

Both have `subject_type: "line"`, `diff_hunk: ""`, `position: 1`, `original_position: 1`. GraphQL shows the first as a `LINE` thread with `diffSide: RIGHT`, `line: 3949`, `diffHunk: ""`. All 25 are on the RIGHT side. 49 replies also have an empty `diff_hunk` (replies inherit their thread's anchor).

Inferred, not documented: these were made in the web UI. The empty `diff_hunk` fits the third 422 message above ("diff hunk can't be blank"): the public write path seems to need the line to sit in a hunk so it can store one, and the web UI uses some other path. `position: 1` is a placeholder, not a real diff position.

## What this means for Tsugiki

1. **Composer.** Keep the current behaviour: no line comment on an expanded line, and no range that reaches into one. A range must also stay inside one hunk (second 422 message). The reason for scope.md: GitHub's API takes line comments only on lines of the diff; the web UI's "comment anywhere" has no public API (changelog 2025-09-25: "API support is currently limited"). If GitHub adds API support later, the change would be what the ticket describes: open the composer on expanded lines and send `line` = the line number at the PR's head with `side: RIGHT`.
2. **Errors.** If a comment is refused anyway (the file changed under the user, for example), REST gives 422 with the messages above, but GraphQL `addPullRequestReviewThread` gives `thread: null` and no error. Treat a null thread as a failure and keep the draft.
3. **Fallback.** A file-level comment (`subject_type: file` / `subjectType: FILE`) is accepted on any file in the diff and could be offered for an expanded line, quoting the line number in the body. Inferred; not tested.
4. **Showing others' comments.** Threads written on github.com on unchanged lines will arrive with `diff_hunk: ""` and `position: 1`, on lines that are not in any hunk. Tsugiki must place them by `path` + `line`/`start_line` + `side` and the comment's commit, not by `diff_hunk` or `position`. Since they sit outside the hunks, the view has to expand those lines (or list such threads at the top of the file) so they are not lost. The classic github.com page hides them behind a warning; Tsugiki should not.
