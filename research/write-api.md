# What GitHub's API takes and refuses when Tsugiki writes

Research for [tsugiki/tsugiki#363](https://github.com/tsugiki/tsugiki/issues/363). Checked 2026-11-10.

## Question

For each write that stage 3 makes, what does GitHub's API accept, what does it refuse, and how does each
refusal look? The writes: comments on an **Issue** and on a PR's conversation, replies to review threads,
resolve and unresolve, the **Pending review** (start, threads on a line, a range or a file, adding to one
begun on github.com, editing and deleting drafts, submitting), `suggestion` blocks, and what each write costs.

Line comments outside a diff hunk are covered by the earlier
[research](https://github.com/tsugiki/tsugiki/blob/research/expanded-line-comments/research/expanded-line-comments.md)
on expanded lines and are not repeated here.

## Sources and method

| Source | Link |
|---|---|
| GitHub's public GraphQL schema (fetched 2026-11-10; `gh api graphql` introspection agrees with it) | [schema.docs.graphql](https://docs.github.com/public/fpt/schema.docs.graphql) |
| GraphQL reference for pulls | [graphql/reference/pulls](https://docs.github.com/en/graphql/reference/pulls) |
| GraphQL breaking changes | [graphql/overview/breaking-changes](https://docs.github.com/en/graphql/overview/breaking-changes) |
| GraphQL changelog | [graphql/overview/changelog](https://docs.github.com/en/graphql/overview/changelog) |
| GraphQL rate limits and query limits | [rate-limits-and-query-limits-for-the-graphql-api](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api) |
| Forming calls with GraphQL | [forming-calls-with-graphql](https://docs.github.com/en/graphql/guides/forming-calls-with-graphql) |
| REST: pull request reviews | [rest/pulls/reviews](https://docs.github.com/en/rest/pulls/reviews) |
| REST: pull request review comments | [rest/pulls/comments](https://docs.github.com/en/rest/pulls/comments) |
| REST: issue comments | [rest/issues/comments](https://docs.github.com/en/rest/issues/comments) |
| REST: API versions (the pages show `2026-03-10`; a request without `X-GitHub-Api-Version` gets `2022-11-28`) | [api-versions](https://docs.github.com/en/rest/about-the-rest-api/api-versions) |
| REST rate limits | [rate-limits-for-the-rest-api](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) |
| REST best practices | [best-practices-for-using-the-rest-api](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api) |
| Troubleshooting the REST API | [troubleshooting-the-rest-api](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api) |
| Scopes for OAuth apps (classic and `gh` tokens) | [scopes-for-oauth-apps](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps) |
| Managing your personal access tokens | [managing-your-personal-access-tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) |
| Permissions required for fine-grained personal access tokens | [permissions-required-for-fine-grained-personal-access-tokens](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens) |
| Commenting on a pull request | [commenting-on-a-pull-request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/commenting-on-a-pull-request) |
| Reviewing proposed changes in a pull request | [reviewing-proposed-changes-in-a-pull-request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request) |
| Incorporating feedback in your pull request | [incorporating-feedback-in-your-pull-request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/incorporating-feedback-in-your-pull-request) |
| Locking conversations | [locking-conversations](https://docs.github.com/en/communities/moderating-comments-and-conversations/locking-conversations) |
| Archiving repositories | [archiving-repositories](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories) |
| `gh`, GitHub's CLI: how it posts comments and reviews | [api/queries_comments.go](https://github.com/cli/cli/blob/trunk/api/queries_comments.go), [api/queries_pr_review.go](https://github.com/cli/cli/blob/trunk/api/queries_pr_review.go) |
| GitHub's MCP server (commit `b98d115`, 2026-10-21): how it builds and submits a pending review | [pkg/github/pullrequests.go](https://github.com/github/github-mcp-server/blob/b98d115bbb080e9555466a96656ec4df605aea21/pkg/github/pullrequests.go) |

Read-only GraphQL queries run on 2026-11-10 show the error shapes and the `viewerCan…` fields quoted below.
Nothing was written to GitHub.

## Answer in short

- Every write Tsugiki needs has a GraphQL mutation. Resolve and unresolve exist **only** in GraphQL. The REST
  write endpoints cover the rest and are the only place GitHub documents status codes, but not the rules.
- A Pending review is a `PullRequestReview` in state `PENDING`, visible only to its author. Threads go in with
  `addPullRequestReviewThread` (line, range or file); replies can go in with `addPullRequestReviewThreadReply`
  and its `pullRequestReviewId`. `submitPullRequestReview` sends it as `APPROVE`, `REQUEST_CHANGES` or
  `COMMENT`. One begun on github.com is the same object, found with `reviews(states: [PENDING])`.
- A `suggestion` block is only Markdown in the body. GitHub has no field for it; the lines it replaces are the
  comment's own line or range.
- Every payload field of every mutation is nullable in the schema, so the schema can't say which ones come
  back null instead of raising an error. Only `addPullRequestReviewThread`'s null `thread` is confirmed.
- Refusals: a classic or `gh` token without the scope should fail before anything runs, with
  `type: INSUFFICIENT_SCOPES` and no `data`, as it does on reads. A fine-grained token without write gets
  "Resource not accessible by personal access token" (documented for REST; its GraphQL shape is untested).
  Locked and archived conversations show up in advance through `locked`, `isArchived` and
  `viewerCannotUpdateReasons`.
- Cost: a GraphQL call costs at least 1 point of the 5,000 an hour. Writes are bound by the secondary limits:
  each mutation counts 5 points against 2,000 a minute, and content creation is capped at about 80 a minute
  and 500 an hour, web UI included. GitHub asks for a pause of at least 1 second between writes.
- The docs say nothing about what a moving head does to a Pending review, what a merged or closed PR refuses,
  or whether a reply without a review ID lands in the Pending review. Those need the live tries at the end.

## The mutations

All of these are in the schema and none is deprecated, except where noted. Each takes an `input` object and
returns a payload; every payload field is nullable.

| Write | GraphQL mutation | Key input | Payload field to check | REST counterpart |
|---|---|---|---|---|
| Comment on an Issue or PR conversation | `addComment` | `subjectId` (Issue or PullRequest), `body` | `commentEdge.node` | `POST /repos/{o}/{r}/issues/{n}/comments` |
| Edit that comment | `updateIssueComment` | `id`, `body` | `issueComment` | `PATCH /repos/{o}/{r}/issues/comments/{id}` |
| Delete that comment | `deleteIssueComment` | `id` | none (payload has only `clientMutationId`) | `DELETE /repos/{o}/{r}/issues/comments/{id}` |
| Start a Pending review | `addPullRequestReview` without `event` | `pullRequestId`, optional `commitOID`, `body`, `threads` | `pullRequestReview` | `POST /repos/{o}/{r}/pulls/{n}/reviews` without `event` |
| Thread on a line, range or file | `addPullRequestReviewThread` | `pullRequestReviewId` or `pullRequestId`, `path`, `line`, `side`, `startLine`, `startSide`, `subjectType`, `body` | `thread` | `POST /repos/{o}/{r}/pulls/{n}/comments` (posts at once, not into a Pending review) |
| Reply to a thread | `addPullRequestReviewThreadReply` | `pullRequestReviewThreadId`, optional `pullRequestReviewId`, `body` | `comment` | `POST /repos/{o}/{r}/pulls/{n}/comments/{id}/replies` |
| Edit a draft or posted review comment | `updatePullRequestReviewComment` | `pullRequestReviewCommentId`, `body` | `pullRequestReviewComment` | `PATCH /repos/{o}/{r}/pulls/comments/{id}` |
| Delete a draft or posted review comment | `deletePullRequestReviewComment` | `id` | `pullRequestReviewComment`, `pullRequestReview` | `DELETE /repos/{o}/{r}/pulls/comments/{id}` |
| Edit the review's summary | `updatePullRequestReview` | `pullRequestReviewId`, `body` | `pullRequestReview` | `PUT /repos/{o}/{r}/pulls/{n}/reviews/{id}` |
| Discard the Pending review | `deletePullRequestReview` | `pullRequestReviewId` | `pullRequestReview` | `DELETE /repos/{o}/{r}/pulls/{n}/reviews/{id}` |
| Submit | `submitPullRequestReview` | `pullRequestReviewId` or `pullRequestId`, `event`, optional `body` | `pullRequestReview` | `POST /repos/{o}/{r}/pulls/{n}/reviews/{id}/events` |
| Resolve, unresolve | `resolveReviewThread`, `unresolveReviewThread` | `threadId` (and `resolutionReason`, for Copilot reviews) | `thread` | none |

Sources: the schema; REST endpoints from the three REST reference pages above. `resolutionReason` is described
as "The reason a Copilot code review thread was resolved" and was added on 2026-08-25
([GraphQL changelog](https://docs.github.com/en/graphql/overview/changelog)).

Avoid `addPullRequestReviewComment` and the `comments` input of `addPullRequestReview`. Every input field of
the first and the `comments` field of the second carry "Upcoming Change on 2023-10-01", "use
addPullRequestReviewThread or addPullRequestReviewThreadReply instead", "We are deprecating comment fields
that use diff-relative positioning"
([breaking changes](https://docs.github.com/en/graphql/overview/breaking-changes), "Changes scheduled for
2023-10-01"). They still exist three years later, but are marked for removal.

`gh` itself posts comments with `addComment` and reviews with `addPullRequestReview` plus `event`, in one call
([queries_comments.go](https://github.com/cli/cli/blob/trunk/api/queries_comments.go),
[queries_pr_review.go](https://github.com/cli/cli/blob/trunk/api/queries_pr_review.go)).

## Comments, replies, resolve

### Comment on an Issue or a PR's conversation

`addComment` "Adds a comment to an Issue or Pull Request". `subjectId` takes either node type. REST: "Every
pull request is an issue, but not every issue is a pull request", and `POST .../issues/{n}/comments` answers
201, 403, 404, 410, or 422 "Validation failed, or the endpoint has been spammed"; the page does not say what
causes the 403 or the 410
([issue comments](https://docs.github.com/en/rest/issues/comments#create-an-issue-comment)). The issues page
says a 410 means the issue was deleted from a repository the user can read, or issues are disabled
([issues](https://docs.github.com/en/rest/issues/issues#get-an-issue),
[create an issue](https://docs.github.com/en/rest/issues/issues#create-an-issue)). The endpoint "triggers
notifications" and "Creating content too quickly using this endpoint may result in secondary rate limiting".

Fine-grained permissions: `POST /repos/{owner}/{repo}/issues/{issue_number}/comments` is listed under both
"Issues: write" and "Pull requests: write", with "Additional permissions" ticked
([permissions](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens)).
The table does not say which of the two an Issue needs and which a PR needs. ADR 0004's fine-grained token has
both, so it doesn't matter yet.

### Reply to a review thread

`addPullRequestReviewThreadReply` "Adds a reply to an existing Pull Request Review Thread". Its
`pullRequestReviewId` is "The Node ID of the pending review to which the reply will belong". The schema does
not say what happens when it is left out while the user has a Pending review. Live try 4.

REST: `POST .../pulls/{n}/comments/{comment_id}/replies` takes "the ID of a top-level review comment, not a
reply to that comment. Replies to replies are not supported." Status codes listed: 201 and 404 only
([create a reply](https://docs.github.com/en/rest/pulls/comments#create-a-reply-for-a-review-comment)).

`PullRequestReviewThread.viewerCanReply` says in advance whether the viewer may reply. A read of an open
`rust-lang/rust` PR (viewer has `READ`) gave `viewerCanReply: true`, `viewerCanResolve: false`.

### Resolve and unresolve

`resolveReviewThread` "Marks a review thread as resolved"; `unresolveReviewThread` the reverse. Both return
`thread`. No REST endpoint exists: neither REST page mentions resolving, and the fine-grained table lists no
resolve endpoint.

Who may: "You can resolve a conversation in a pull request if you opened the pull request or if you have write
access to the repository where the pull request was opened"
([commenting](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/commenting-on-a-pull-request)).
`viewerCanResolve` and `viewerCanUnresolve` on the thread say it per thread. The docs don't say what resolving
an already resolved thread returns. GitHub's MCP server checks only for errors after these two mutations, not
for a null thread
([pullrequests.go](https://github.com/github/github-mcp-server/blob/b98d115bbb080e9555466a96656ec4df605aea21/pkg/github/pullrequests.go)).

## The Pending review

### What it is

"Before you submit your review, your line comments are *pending* and only visible to you. You can edit pending
comments anytime before you submit your review. To cancel a pending review, including all of its pending
comments, click Submit review, then click Discard review"
([reviewing](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request)).
In the schema it is a `PullRequestReview` with `state: PENDING` ("A review that has not yet been submitted"),
and its comments have `PullRequestReviewComment.state: PENDING` ("A comment that is part of a pending
review"). `PullRequest.latestReviews` skips reviews that are "also pending review".

### Starting one

`addPullRequestReview` with no `event` creates it. REST says the same of its endpoint: "To create a pending
review for a pull request, leave the event parameter blank" and "Pull request reviews created in the PENDING
state are not submitted and therefore do not include the submitted_at property"
([create a review](https://docs.github.com/en/rest/pulls/reviews#create-a-review-for-a-pull-request)).

`commitOID` is "The commit OID the review pertains to". REST's `commit_id` "Defaults to the most recent commit
in the pull request when you do not specify a value", and "Not using the latest commit SHA may render your
review comment outdated if a subsequent commit modifies the line you specify as the position".

The `threads` input (`DraftPullRequestReviewThread`) can create the review and its threads in one call. It has
`path`, `line`, `side`, `startLine`, `startSide`, `body`, but **no `subjectType`**, so a file thread can't be
created this way; it needs `addPullRequestReviewThread`.

The docs never say a user can have only one Pending review on a PR, and never say what a second
`addPullRequestReview` without `event` does. Live try 3.

### Finding one begun on github.com

It is the same object, so Tsugiki adds to it as to its own. `PullRequest.reviews` takes
`states: [PullRequestReviewState!]` and `author: String`. GitHub's MCP server finds the viewer's pending
review by paging `reviews(first: 100, states: [PENDING])` and comparing each author's ID with `viewer.id`
([`getPendingPullRequestReviewForViewer`](https://github.com/github/github-mcp-server/blob/b98d115bbb080e9555466a96656ec4df605aea21/pkg/github/pullrequests.go)).
The docs don't say whether `viewerLatestReview` can be a pending review.

### Adding a thread

`addPullRequestReviewThread` "Adds a new thread to a pending Pull Request Review". Inputs:

- `pullRequestReviewId`: "The Node ID of the review to modify." `pullRequestId`: "The node ID of the pull
  request reviewing." Both optional. Which wins when both are given, and whether `pullRequestId` alone creates
  a Pending review when none exists, is not documented. Live try 2.
- `path`: "Path to the file being commented on."
- `line`: "The line of the blob to which the thread refers, required for line-level threads. The end of the
  line range for multi-line comments."
- `side: DiffSide = RIGHT`: "The side of the diff on which the line resides. For multi-line comments, this is
  the side for the end of the line range."
- `startLine`: "The first line of the range to which the comment refers." `startSide: DiffSide = RIGHT`: "The
  side of the diff on which the start line resides."
- `subjectType: PullRequestReviewThreadSubjectType = LINE`: `LINE` or `FILE` ("A comment that has been made
  against the file of a pull request").

There is no `commitOID`, and the docs don't say which commit the thread is placed against: the review's, or
the PR's head. Live try 6.

REST adds a few rules for the same fields. `line` is "Required unless using subject_type:file"; `start_line`
and `start_side` are "Required when using multi-line comments unless using in_reply_to"; `side`: "Use LEFT for
deletions that appear in red. Use RIGHT for additions that appear in green or unchanged lines that appear in
white and are shown for context. For a multi-line comment, side represents whether the last line of the
comment range is a deletion or addition"
([REST](https://docs.github.com/en/rest/pulls/comments#create-a-review-comment-for-a-pull-request)). So
`startSide` and `side` may differ: a range can start on a removed line and end on an added one.

What the docs leave out: whether `startLine` must be below `line`, whether `startLine == line` is accepted,
and whether a `FILE` thread ignores a `line` it is given. The earlier research found the one rule they imply:
the whole range must sit inside one hunk (REST 422 "Pull request review thread start line must be part of the
same hunk as the line."). Live try 5.

File threads: "Commenting on files (including deleted, binary, and renamed files) in a pull request is now
generally available ... API support is also now available", via `addPullRequestReviewThread` or REST, and "A
new "subject type" field is also now returned by other APIs" (2023-04-11
[changelog](https://github.blog/changelog/2023-04-11-commenting-on-files-in-a-pull-request-is-now-generally-available/)).

**A null thread has three known causes.** GitHub's MCP server, after `addPullRequestReviewThread`, checks
`Thread.ID == nil` and reports: "The line number doesn't exist in the pull request diff", "The file path is
incorrect", "The side (LEFT/RIGHT) is invalid for the specified line"
([pullrequests.go](https://github.com/github/github-mcp-server/blob/b98d115bbb080e9555466a96656ec4df605aea21/pkg/github/pullrequests.go)).
That is GitHub's own code reading a null thread as a refusal with any of three causes, and with no error to
tell them apart.

### Editing and deleting drafts

`updatePullRequestReviewComment` ("Updates a pull request review comment") and
`deletePullRequestReviewComment` ("Deletes a pull request review comment") take any review comment's node ID;
the schema doesn't limit them to pending or submitted ones. The delete payload returns both the deleted
`pullRequestReviewComment` and "The pull request review the deleted comment belonged to". Whether deleting the
last draft leaves an empty Pending review is not documented. Live try 9.

`updatePullRequestReview` changes the summary body (REST: "Updates the contents of a specified review summary
comment"). `deletePullRequestReview` discards the whole Pending review; REST: "Deletes a pull request review
that has not been submitted. Submitted reviews cannot be deleted," with codes 200, 404, 422
([REST](https://docs.github.com/en/rest/pulls/reviews#delete-a-pending-review-for-a-pull-request)). GitHub's
MCP server deletes the viewer's pending review with this mutation.

Per comment, `viewerCanUpdate`, `viewerCanDelete` and `viewerCannotUpdateReasons` say in advance what is
allowed.

### Submitting

`submitPullRequestReview` "Submits a pending pull request review". `event: PullRequestReviewEvent!` is
required; the enum has `APPROVE`, `COMMENT`, `REQUEST_CHANGES` and `DISMISS` ("Dismiss review so it now longer
effects merging"), which only fits `dismissPullRequestReview`. `body` is optional ("The text field to set on
the Pull Request Review"). `pullRequestId` is "The Pull Request ID to submit any pending reviews", an
alternative to `pullRequestReviewId`.

REST, same action: "When you leave this blank, the API returns HTTP 422 (Unrecognizable entity) and sets the
review action state to PENDING", codes 200, 403, 404, 422
([submit a review](https://docs.github.com/en/rest/pulls/reviews#submit-a-review-for-a-pull-request)). When a
review is created and submitted in one call, `body` is "Required when using REQUEST_CHANGES or COMMENT for the
event parameter"
([create a review](https://docs.github.com/en/rest/pulls/reviews#create-a-review-for-a-pull-request)). Whether
that rule also holds at submit time, and whether a `COMMENT` with threads but no body passes, is not said.
Live try 7.

Rules in the product docs: "Pull request authors cannot approve their own pull requests"; "The Request changes
option is purely informational and will not prevent merging unless a ruleset or classic branch protection rule
is configured"; "If both required reviews and stale review dismissal are enabled and a code-modifying commit
is pushed to the branch of an approved pull request, the approval is dismissed"
([reviewing](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request)).
They don't say whether an author may submit `REQUEST_CHANGES` on their own PR. Notifications: "Batching your
comments avoids sending multiple notifications"
([commenting](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/commenting-on-a-pull-request)),
so drafts notify nobody until the submit.

### When the PR's head moves

The docs say only that a comment made against an older commit may become outdated when "a subsequent commit
modifies the line" (REST `commit_id` above), and the schema has `PullRequestReview.commit` ("Identifies the
commit associated with this pull request review"), `PullRequestReviewThread.isOutdated` ("Indicates whether
this thread was outdated by newer changes"), and `originalLine` beside `line`. Nothing says:

- whether a Pending review stays pinned to the commit it started on, or follows the head;
- which commit's line numbers a new `addPullRequestReviewThread` is read against after the head moved;
- what `commit` a submitted review ends up with.

This decides whether Tsugiki's line numbers stay valid while a review is open. Live try 6.

## `suggestion` blocks

GitHub needs nothing but the Markdown. The schema has no suggestion field on any review input; the only
related field is the read-only `PullRequest.viewerCanApplySuggestion` ("Whether or not the viewer can apply
suggestion"). The docs describe the block only through the web UI: "The lines you selected are copied into the
comment box, where you can edit them to suggest a change. You can add a comment above the line
containing ```` ```suggestion ```` to explain your suggested change"
([reviewing](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/reviewing-proposed-changes-in-a-pull-request)).
So the block's content replaces the comment's own lines: `line` alone, or `startLine` to `line`.

Applying is a separate act by someone with write access: "Applying one suggested change or a batch of
suggested changes creates a single commit on the compare branch"; each suggester becomes a co-author
([incorporating](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/incorporating-feedback-in-your-pull-request)).
Multi-line suggestions have been generally available since 2020 (2020-04-15
[changelog](https://github.blog/changelog/2020-04-15-multi-line-code-suggestions-general-availability/)).

Silent: suggestions on `LEFT` (removed) lines, on a range that mixes sides, more than one block in a comment,
a block in a file thread or an issue comment, and whether GitHub ever refuses a body because of its block.
Since the API takes any body, expect GitHub to accept all of these and render some of them as plain code
blocks instead of applicable suggestions. Live try 13.

## Refusals

### How a GraphQL refusal looks

GraphQL answers HTTP 200 with an `errors` array. Each error carries a `type`, a `message`, `path` and
`locations`, and `data` holds `null` where the failed field was. GitHub doesn't document the list of `type`
values. Seen on 2026-11-10 with read-only queries:

- `NOT_FOUND`, for a repository that doesn't exist: `"data":{"a":null}` and
  `"errors":[{"type":"NOT_FOUND","path":["a"],"message":…}]` (`locations` left out), the message being "Could
  not resolve to a Repository with the name '…'."
- `INSUFFICIENT_SCOPES`, for `viewer { email }` with gh's token (`gist, read:org, repo, workflow`): no `data`
  key at all, and the message names the field, the scopes it needs and the scopes the token has: "Your token
  has not been granted the required scopes to execute this query. The 'email' field requires one of the
  following scopes: ['user:email', 'read:user'], but your token has only been granted the: ['gist',
  'read:org', 'repo', 'workflow'] scopes."
- `INTROSPECTION_LIMIT_EXCEEDED`, for a query that used `__Type.fields` 28 times.

GitHub checks scopes before it runs anything, on the whole document. A mutation should fail the same way, so a
missing scope can't leave a write half done. That is inferred from the read case; live try 10 checks it on a
mutation.

Errors that GitHub raises from inside a mutation (validation, a locked conversation, a closed PR) have no
documented `type`. REST's equivalents are 403 and 422 (below).

### Token kinds

- **Classic or `gh` token without `public_repo`.** "(no scope)" "Grants read-only access to public
  information"; `public_repo` "Limits access to public repositories. That includes read/write access to code,
  commit statuses, repository projects, collaborators, and deployment statuses for public repositories"
  ([scopes](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps)). The
  `X-OAuth-Scopes` header lists the token's scopes on every response (on 2026-11-10:
  `X-Oauth-Scopes: gist, read:org, repo, workflow`), so Tsugiki can know up front, as ADR 0004 says. A write
  without the scope should get `INSUFFICIENT_SCOPES` (shape above). REST answers such a write with 404 for
  private resources: "GitHub uses a 404 Not Found response instead of a 403 Forbidden response to avoid
  confirming the existence of private repositories"
  ([troubleshooting](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api#404-not-found-for-an-existing-resource)).
- **Fine-grained token without write.** REST: "If you are using a GitHub App or fine-grained personal access
  token and you receive a "Resource not accessible by integration" or "Resource not accessible by personal
  access token" error, then your token has insufficient permissions." The `X-Accepted-GitHub-Permissions`
  header names what the endpoint needs, such as `pull_requests=write,contents=read`
  ([troubleshooting](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api#resource-not-accessible)).
  GraphQL: "If your token does not have the required scopes or permissions to access a resource, the API will
  return an error message that states the scopes or permissions your token needs"
  ([forming calls](https://docs.github.com/en/graphql/guides/forming-calls-with-graphql)); the `type` and the
  exact message are not documented. Live try 10. Three cases give the same refusal: a token whose repository
  selection leaves the Repository out ("Tokens always include read-only access to all public repositories on
  GitHub"); a token still `pending` an organization's approval ("Your token will only be able to read public
  resources until it is approved"); and a user who isn't a member of the owner ("Only personal access tokens
  (classic) have write access for public repositories that are not owned by you or an organization that you
  are not a member of")
  ([tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)).
  `Repository.viewerPermission` is "The users permission level on the repository", the user's and not the
  token's, so it probably can't warn of these. Live try 10 checks it.
- **Locked conversation.** "While a conversation is locked, only people with write access and repository
  owners and collaborators can add, hide, and delete comments. Reactions and votes in a locked conversation
  are disabled for all users"
  ([locking](https://docs.github.com/en/communities/moderating-comments-and-conversations/locking-conversations)).
  A core developer with write access can still comment. It shows in advance: `locked`, `activeLockReason`, and
  `viewerCannotUpdateReasons`, whose enum includes `LOCKED` ("Unable to create comment because issue is
  locked"). A read of a locked `rust-lang/rust` PR (viewer `READ`) gave `locked: true`,
  `viewerCannotUpdateReasons: ["LOCKED", "INSUFFICIENT_ACCESS"]` and `viewerCanReact: false`. The refusal
  itself is undocumented; REST's create-comment lists 403. Live try 11.
- **Archived Repository.** "Its issues, pull requests, code, labels, milestones, projects, wiki, releases,
  commits, tags, branches, reactions, code scanning alerts, comments and permissions become read-only"
  ([archiving](https://docs.github.com/en/repositories/archiving-a-github-repository/archiving-repositories)),
  and "Conversations are automatically locked in archived repositories"
  ([locking](https://docs.github.com/en/communities/moderating-comments-and-conversations/locking-conversations)).
  `Repository.isArchived` says it up front. A read of a PR in the archived `github/ai-moderator` gave
  `isArchived: true`, the PR `locked: true`, and its comments
  `viewerCannotUpdateReasons: ["ARCHIVED", "LOCKED", "INSUFFICIENT_ACCESS"]` (`ARCHIVED`: "Unable to create
  comment because repository is archived"). The refusal's shape is undocumented. Live try 12.
- **PR merged or closed meanwhile.** The docs say nothing about comments, reviews or threads on a closed or
  merged PR. A read of the merged tsugiki/tsugiki#358 (viewer `ADMIN`) gave `viewerCanUpdate: true` and empty
  `viewerCannotUpdateReasons`, which suggests a merged PR still takes comments. Live try 8.
- **The thread, comment or review was deleted meanwhile.** Presumably `NOT_FOUND` on the node ID, as for the
  repository above. REST answers 404. Not documented for mutations.

### Rate limits

- **Primary, GraphQL.** "5,000 points per hour per user", including personal access tokens and OAuth apps
  acting for a user. Exceeding it: "the response status will still be 200, but you will receive an error
  message, and the value of the x-ratelimit-remaining header will be 0. You should not retry your request
  until after the time specified by the x-ratelimit-reset header"
  ([limits](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api)).
- **Primary, REST.** A separate budget. Exceeding it: "a 403 or 429 response, and the x-ratelimit-remaining
  header will be 0"
  ([REST limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)).
- **Secondary**, shared by REST and GraphQL: no more than 100 concurrent requests; "No more than 900 points
  per minute are allowed for REST API endpoints, and no more than 2,000 points per minute are allowed for the
  GraphQL API endpoint"; CPU time (90 s per minute, 60 of it GraphQL); and "In general, no more than 80
  content-generating requests per minute and no more than 500 content-generating requests per hour are
  allowed. Some endpoints have lower content creation limits. Content creation limits include actions taken on
  the GitHub web interface as well as via the REST API and GraphQL API." "These secondary rate limits are
  subject to change without notice. You may also encounter a secondary rate limit for undisclosed reasons."
- **Secondary, the answer.** GraphQL: "the response status will be 200 or 403, and you will receive an error
  message that indicates that you hit a secondary rate limit. If the retry-after response header is present,
  you should not retry your request until after that many seconds has elapsed. If the x-ratelimit-remaining
  header is 0, you should not retry your request until after the time ... specified by the x-ratelimit-reset
  header. Otherwise, wait for at least one minute before retrying", with exponential backoff after that. REST:
  403 or 429, same headers. "Continuing to make requests while you are rate limited may result in the banning
  of your integration." There is "not a way to check the status of your secondary rate limit."
- **Pacing.** "pause at least 1 second between mutative requests and avoid concurrent requests" (GraphQL
  page); "make requests serially instead of concurrently"
  ([practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)).
- **Timeouts.** "If GitHub takes more than 10 seconds to process an API request, GitHub will terminate the
  request and you will receive a timeout response", and "additional points will be deducted from your primary
  rate limit for the next hour". The docs don't say whether a write cut off this way was applied.
  `clientMutationId` is only "A unique identifier for the client performing the mutation" and is echoed back;
  nothing says GitHub uses it to drop a repeat. So after a timeout or a dropped connection, the outcome of a
  write is unknown until it is read back.

## What each write costs

| Budget | Per write | Source |
|---|---|---|
| GraphQL primary (5,000/h) | At least 1 point; a payload that selects connections can cost more. Mutation costs aren't listed separately. `rateLimit` is a `Query` field, so a mutation can't ask its own cost; the `x-ratelimit-used` header can. | [GraphQL limits](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api): "The minimum point value of a call to the GraphQL API is 1" |
| REST primary (separate budget) | 1 request | [REST limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) |
| Secondary points (GraphQL 2,000/min, REST 900/min) | 5 for a GraphQL request with mutations, 5 for most REST `POST`, `PATCH`, `PUT`, `DELETE` | same pages, "Calculating points for the secondary rate limit" |
| Content creation (80/min, 500/h, web UI included) | 1 per content-generating request; the docs don't define which writes count | same pages |

For a sense of scale: a review with 30 threads is about 32 writes (start, 30 threads, submit). With a 1-second
pause that takes over half a minute and stays under the 80 a minute. Its points are noise next to the 5,000 an
hour, but a user who reviews several large PRs in an hour, on the web as well, could near the 500 content
requests. Whether `addPullRequestReview` with all its `threads` in one call counts as one content request is
not documented.

## Third-party reports (leads for the live tries, not evidence)

Reports from other projects' issue trackers, unverified. They are listed only because each one names a live
try:

- A REST reply (`POST .../comments/{id}/replies`) while the user has a Pending review fails with
  `422 Validation Failed: user_id can only have one pending review per pull request`, and
  `addPullRequestReviewThreadReply` with `pullRequestReviewId` adds a `PENDING` reply to that review
  ([dgowrie/claude-workflows#148](https://github.com/dgowrie/claude-workflows/issues/148), 2026-10-08).
- A Pending review's comments are missing from REST `GET .../pulls/{n}/comments` but present in GraphQL
  `reviewThreads`, and REST replies fail with the same 422
  ([seasonedcc/seasoned-skills#451](https://github.com/seasonedcc/seasoned-skills/issues/451), 2026-09-29).
- A body set on a Pending review through REST was lost when the user submitted it from the web UI, and
  `PUT .../reviews/{id}` then answered `422 "Could not edit a review with a missing body."`
  ([dgowrie/claude-workflows#144](https://github.com/dgowrie/claude-workflows/issues/144), 2026-10-06).

## Live tries left for the maintainer

All on a throwaway PR in tsugiki/tsugiki unless noted, with `gh api -i graphql` so the headers show. Record
the full response each time: HTTP status, `data`, `errors[].type`, `errors[].message`, and `x-ratelimit-used`
before and after.

1. **Cost.** For each mutation in the table, compare `x-ratelimit-used` (resource `graphql`) before and after.
   Do the same for one REST `POST .../issues/{n}/comments` against resource `core`.
2. **`pullRequestId` alone.** With no Pending review:
   `addPullRequestReviewThread(input: {pullRequestId, path, line, side: RIGHT, body})`. Does it create a
   Pending review, post a single comment, or return a null `thread`? Repeat with a Pending review open: does
   the thread join it?
3. **One Pending review.** `addPullRequestReview(input: {pullRequestId})` twice. Does the second fail (type,
   message), or return the existing review? Repeat while a Pending review begun on github.com is open.
4. **Replies.** `addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId, body})` without
   `pullRequestReviewId`, (a) with no Pending review: posted at once? (b) with one: posted at once, joined to
   it, or refused? Then with `pullRequestReviewId`: is `comment.state` `PENDING`, and does it go out on
   submit? Also the REST reply in case (b), to confirm the 422 above.
5. **Ranges and files.** `addPullRequestReviewThread` with (a) `startLine == line`; (b) `startLine > line`;
   (c) `startSide: LEFT, side: RIGHT` inside one hunk; (d) a wrong `path`; (e) `side: LEFT` on an added line;
   (f) `subjectType: FILE` with and without `line`; (g) `subjectType: FILE` on a deleted, a renamed and a
   binary file. For each: a thread, a null `thread`, or an error?
6. **Head moves.** Start a Pending review at head A with a thread on line N. Push commit B, once changing line
   N and once leaving it. Then read `review.commit.oid`, the thread's `line`, `originalLine`, `isOutdated`.
   Add a new thread using B's line numbers: is it placed against A or B? Submit, and read `review.commit.oid`
   again.
7. **Submit.** `submitPullRequestReview` with (a) `COMMENT`, threads, no `body`; (b) `COMMENT`, no threads, no
   `body`; (c) `REQUEST_CHANGES` without `body`; (d) `APPROVE` and (e) `REQUEST_CHANGES` by the PR's author;
   (f) `pullRequestId` instead of `pullRequestReviewId`. Also: a body set by `addPullRequestReview`, then
   submit without `body`: is it kept?
8. **Closed and merged.** On a closed and on a merged throwaway PR: `addComment`, `addPullRequestReview`,
   `addPullRequestReviewThread`, `submitPullRequestReview`, `resolveReviewThread`. Also close the PR while a
   Pending review is open, then try to submit.
9. **Drafts.** `updatePullRequestReviewComment` and `deletePullRequestReviewComment` on a pending comment;
   delete the only draft and read whether the Pending review remains; `deletePullRequestReview` on a submitted
   review (expect an error); `deletePullRequestReviewComment` payload: is `pullRequestReviewComment` null
   after a delete that worked?
10. **Tokens.** (a) A classic token with no scopes: `addComment` on the PR. Expect `INSUFFICIENT_SCOPES` with
    no `data`, and confirm nothing was posted. (b) A fine-grained token for tsugiki with Pull requests and
    Issues set to read: `addComment`, `addPullRequestReviewThread`, `resolveReviewThread`, and the REST
    `POST .../issues/{n}/comments`. Record `type`, message and `X-Accepted-GitHub-Permissions`. With the same
    token, read `repository.viewerPermission` and a thread's `viewerCanResolve`: do they reflect the token or
    the user?
11. **Locked.** Needs a second account without write access. Lock the throwaway PR and try `addComment` and
    `addPullRequestReviewThreadReply` from that account. Skip if no such account is at hand: Tsugiki can read
    `locked` and `viewerPermission` first.
12. **Archived.** Needs a throwaway repository under the maintainer's account, not tsugiki/tsugiki. Archive it
    and try `addComment`. Skippable for the same reason: `isArchived` is known up front.
13. **Suggestions.** Threads whose body holds a ```` ```suggestion ```` block: on a `LEFT` line, on a range
    across sides, two blocks in one body, in a `FILE` thread, and in an `addComment`. Does GitHub accept each,
    and which render as an applicable suggestion on github.com?
14. **Body size.** `addComment` with a body of 65,537 characters. The limit isn't documented anywhere.
15. **Resolve edge cases.** `resolveReviewThread` on a thread that is already resolved, and on the user's own
    thread that is still in the Pending review.

Not worth trying: the secondary limits. Provoking them risks a ban ("Continuing to make requests while you are
rate limited may result in the banning of your integration").

## What this means for Tsugiki

### Mutations to use

- Comment on an Issue or a PR's conversation: `addComment`. Edit and delete: `updateIssueComment`,
  `deleteIssueComment`.
- Start the Pending review:
  `addPullRequestReview(input: {pullRequestId, commitOID: <the head Tsugiki shows>})` without `event`, only
  when `reviews(states: [PENDING])` holds none by the viewer. Otherwise add to the one found, whether Tsugiki
  or github.com started it.
- Every new thread: `addPullRequestReviewThread` with `pullRequestReviewId`, `path`, `line`, `side`, and for a
  range `startLine`, `startSide`; `subjectType: FILE` for a file thread. Not the `threads` input, which can't
  make a file thread, and not the deprecated `comments` input or `addPullRequestReviewComment`.
- Replies: `addPullRequestReviewThreadReply`. Whether a reply joins the Pending review or goes out at once is
  a product choice that live try 4 makes possible to decide (#367).
- Edit and delete drafts: `updatePullRequestReviewComment`, `deletePullRequestReviewComment`. Summary:
  `updatePullRequestReview`. Discard: `deletePullRequestReview`.
- Submit: `submitPullRequestReview` with `pullRequestReviewId`, `event`, `body`.
- Resolve and unresolve: `resolveReviewThread`, `unresolveReviewThread`, the only way.
- Stay on GraphQL for all of it, as ADR 0015 does for reads: one budget, one error shape, and the only path
  for resolve and for replies into a Pending review.

### What the core treats as `write_refused`

A write succeeded only when there is no `errors` array **and** the payload holds the object it should:

| Mutation | Success means |
|---|---|
| `addComment` | `commentEdge.node` non-null |
| `addPullRequestReview` | `pullRequestReview` non-null with `state: PENDING` |
| `addPullRequestReviewThread` | `thread` non-null (null: line outside the diff, wrong path or wrong side) |
| `addPullRequestReviewThreadReply` | `comment` non-null |
| `update…` mutations | the updated object non-null |
| `submitPullRequestReview` | `pullRequestReview` non-null and its `state` no longer `PENDING` |
| `resolveReviewThread`, `unresolveReviewThread` | `thread` non-null with `isResolved` as asked |
| `deletePullRequestReview`, `deletePullRequestReviewComment` | no errors; whether the payload object is null after a delete that worked is live try 9 |
| `deleteIssueComment` | no errors (the payload carries nothing) |

The schema makes every payload field nullable, so only `addPullRequestReviewThread`'s null is known from use.
Checking all of them costs nothing and covers the ones not yet seen.

Mapping the rest onto ADR 0008's kinds, as far as the sources go:

- `INSUFFICIENT_SCOPES`, "Resource not accessible by personal access token", a locked or archived refusal →
  `write_refused`, and the Repository becomes **Read-only** as scope.md says. Read `isArchived`, `locked`,
  `viewerPermission` and `viewerCannotUpdateReasons` with the PR so the composer can say so before the user
  types; only a fine-grained token's refusal arrives unannounced.
- A rate-limit error, or 403 or 429 on REST → `rate_limited`, with the reset from `x-ratelimit-reset`, or
  `retry-after`, or one minute when neither is given.
- `NOT_FOUND` on the thread, comment or review → `not_found`. On the PR or Repository, check the token first
  (a private resource answers 404 to a token that can't see it).
- A timeout, a 502 or a dropped connection → the outcome is unknown, not refused. Read back before any retry,
  since GitHub documents no way to drop a repeated write.

### Open questions for #366 (from the composer to GitHub and back)

- Does the core pace writes itself (one at a time, a second apart, as GitHub asks), or share the sync's queue
  from ADR 0015? Writes are the user's work and should go first.
- After an unknown outcome, what does the read-back look for? A thread or reply has no ID until it exists, so
  matching by body and position is the only way, unless the result of live try 2 or 4 offers better.
- Which payload fields does each mutation select, so `changed` can update the view without a second query, and
  is a selection with connections worth the points?
- Where do unsent drafts go when the head moves under an open Pending review, once live try 6 says what GitHub
  does?
- If the user's Pending review is discarded or submitted on github.com meanwhile, the next
  `addPullRequestReviewThread` with the old `pullRequestReviewId` presumably fails. Does the core then start a
  new review and retry, or ask?

### Open questions for #367 (writing a comment, building a Pending review, submitting)

- Do replies join the Pending review, go out at once, or does the user choose?
- Can a line comment go straight out, as github.com's "If you only want to add this one comment, click
  Comment" allows
  ([commenting](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/commenting-on-a-pull-request))?
  That means a review created and submitted in one `addPullRequestReview` call. scope.md says comments go into
  the Pending review; does that cover every comment?
- Should Submit with `COMMENT` require a body? It depends on live try 7.
- Hide Approve and Request changes on the user's own PR, given "Pull request authors cannot approve their own
  pull requests"?
- How is a `suggestion` offered on a removed line or a mixed range, if live try 13 shows GitHub won't render
  it as applicable there?
- What does the composer show for a closed or merged PR, before live try 8 says what GitHub refuses?
