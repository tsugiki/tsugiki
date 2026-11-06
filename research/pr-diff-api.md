# A PR's diff from GitHub's APIs, and where it stops

Research for [#154](https://github.com/tsugiki/tsugiki/issues/154), part of [Plan Tsugiki
0.2.0](https://github.com/tsugiki/tsugiki/issues/152). Checked 2026-11-06 against `python/cpython` with the
user's `gh` token: 25 read-only calls (17 REST, 8 GraphQL), nothing written. "Measured" means a call made for
this note. "Inferred" means reasoned from the docs or from those calls without a direct test. Earlier notes
this builds on: `research/github-sync.md` on the `research/github-sync` branch (rate limits, #9) and
`research/expanded-line-comments.md` on `research/expanded-line-comments` (#5).

## Answer

- **Only REST gives patches.** GraphQL's `PullRequest.files` lists each file's path, change type and line
  counts, and nothing else: no patch, and no old path for a rename. The diff text comes from the REST files
  list (`GET /pulls/{n}/files`, one `patch` per file), from the whole PR as `.diff`/`.patch` (one request), or
  from compare.
- **The whole-PR `.diff` stops at 20,000 lines.** Past that it returns **406** with code `too_large` and
  nothing else. CPython's distutils removal (#99061, 113 files, 22,834 deleted lines) already gets the 406.
- **The files list drops patches in two ways, and neither is documented.**
  1. Each response has a budget of about 20,000 changed lines. Files past it come back with no `patch` **and
     with `additions`, `deletions` and `changes` set to 0**. Ask for the same files on a smaller page and they
     come back whole. This one is fixable: re-request the missing files.
  2. A single file with a large diff gets no `patch`, even on a page of its own, but keeps its real
     counts. Two files with 321 KB and 429 KB diffs (3,694 and 3,977 changed lines) were cut, though GitHub
     documents 500 KB and 20,000 lines per file. The largest patch seen was 52 KB, so the cut lies somewhere
     between those sizes. This one is final: the only way to show that file is to fetch both versions and diff
     them locally.
- **Binary files never have a patch.** They show `changes: 0`, and GraphQL `Blob.isBinary` says which they
  are.
- **Getting a file's whole text is cheap and works for any size CPython has.** GraphQL `object(expression:
  "<oid>:<path>")` returns `Blob.text`, cut at exactly **512,000 bytes** with `isTruncated: true`. One query
  fetched six blobs for 1 point. REST raw (`contents` with the `raw` media type, or `git/blobs/{sha}`) returns
  files up to 100 MB, one request each.
- **The base side is the merge base, and GraphQL can't give it.** GraphQL has no merge-base field. REST
  compare (`base...head`) returns `merge_base_commit` in one request.
- **Cost is no problem, and neither is the timeout.** A 113-file PR costs 2 REST requests for its list, plus
  1–2 to re-request dropped patches. Blobs cost 1 GraphQL point per batch or 1 REST request each, fetched only
  when needed. REST can't list a PR past 3,000 files. No PR checked here came near that, and 0.3's mirror
  covers them.
- **A review thread is anchored by file line numbers, not by diff position.** `diffSide: RIGHT` + `line` is a
  line of the head file. `LEFT` + `line` is a line of the merge-base file, and it moves when the base moves.
  44 of 45 outdated threads checked have `line: null`, and only `originalLine` at `originalCommit` places
  them. A `FILE` thread carries a placeholder `line: 1`. Details and quirks under "Review threads and the
  diff".

For 0.2 this suggests: take the file list and patches from REST; re-request any file that came back with no
patch and 0 counts; for a file that still has no patch, or when the user expands context, fetch both versions
(GraphQL in batches, REST raw past 512 KB) and diff them locally. Tsugiki already depends on the git CLI (ADR
0003), whose `git diff --no-index` reproduced GitHub's counts exactly on the one file checked. Details under
"What this means for 0.2".

## The documented limits

| Where | Limit | Source |
|---|---|---|
| REST: list PR files | "Responses include a maximum of 3000 files"; 30 per page by default, `per_page` up to 100 | [List pull requests files][pulls] |
| REST: list PR files | Fields: `sha`, `filename`, `status` (`added`, `removed`, `modified`, `renamed`, `copied`, `changed`, `unchanged`), `additions`, `deletions`, `changes`, `patch`, `previous_filename`, `blob_url`, `raw_url`, `contents_url` | same |
| REST: get a commit | over 300 files: pagination "up to a limit of 3000 files"; `diff`/`patch`: "Larger diffs may time out and return a 5xx status code"; "Diffs with binary data will have no patch property" | [Commits][commits] |
| REST: compare | 250 commits without paging parameters; "up to 300 changed files for the entire comparison", on the first page only | same |
| REST media types | "the REST API to manage commits and pull requests support the media types `diff`, `patch`, and `sha`" | [Getting started: media types][media] |
| Diffs on github.com | whole PR: 20,000 loadable lines or 1 MB raw diff; one file: 20,000 lines or 500 KB; 300 files; 25 renderable files (images, PDFs, GeoJSON); "anything exceeding the limit is not shown" | [Repository limits][limits] |
| REST: get content | up to 1 MB: everything; 1–100 MB: only the `raw` or `object` media type; over 100 MB: not supported | [Repository contents][contents] |
| REST: get a blob | "supports blobs up to 100 megabytes"; base64, or raw with `application/vnd.github.raw+json` | [Git blobs][blobs] |
| GraphQL `Blob` | `text`: "UTF8 text data or null if the Blob is binary"; `isTruncated`; `isBinary` ("null if unable to determine"); `byteSize`. No truncation size given | [public schema][schema] |
| GraphQL `PullRequestChangedFile` | only `path`, `changeType` (`ADDED`, `CHANGED`, `COPIED`, `DELETED`, `MODIFIED`, `RENAMED`), `additions`, `deletions`, `viewerViewedState` | same |
| GraphQL limits | 5,000 points/hour; a request is cut off after 10 s; cost is the number of connections ÷ 100, minimum 1 | [GraphQL limits][gqllimits]; summed up in `research/github-sync.md` |

The REST pages describe no limit on `patch` itself, and say nothing about what `patch` holds for a file it
leaves out. The 20,000-line and 500 KB numbers are documented for github.com's diff view only.

[pulls]: https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28
[commits]: https://docs.github.com/en/rest/commits/commits?apiVersion=2022-11-28
[media]: https://docs.github.com/en/rest/using-the-rest-api/getting-started-with-the-rest-api
[limits]: https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits
[contents]: https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28
[blobs]: https://docs.github.com/en/rest/git/blobs?apiVersion=2022-11-28
[schema]: https://docs.github.com/public/fpt/schema.docs.graphql
[gqllimits]: https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api

## What the APIs return (measured)

### The whole PR as `.diff`

- `GET /repos/python/cpython/pulls/99061` with `Accept: application/vnd.github.diff` (113 files, +26 −22,834):

  ```
  HTTP/2.0 406 Not Acceptable
  {"message":"Sorry, the diff exceeded the maximum number of lines (20000)",
   "errors":[{"resource":"PullRequest","field":"diff","code":"too_large"}], ...}
  ```

  It counted as 1 request against `core`. No part of the diff comes back.
- #161214 (3 files) returned a normal 1,266-byte git diff. A binary rename shows as `similarity index 54%`,
  `rename from`/`rename to`, an `index d884a0ad81170c1..9b9b4291019bbfa` line (15-character abbreviated blob
  ids) and `Binary files a/... and b/... differ`. The `patch` media type returned the same change as a `git
  format-patch` mail.
- Not tested: the 300-file and 1 MB limits on `.diff`. Inferred from the docs table above that they also give
  406.

### The files list: two kinds of missing patch

**Per response.** `GET /pulls/99061/files?per_page=100`, page 1. The first 86 files hold 19,822 changed lines
and have patches (767 KB of patch text in the response). Files 87–100 have no `patch`, and they report
`additions: 0, deletions: 0, changes: 0`, although they are deleted files (GraphQL gives −107 to −263 for the
four checked). GraphQL gives the real counts for the same file:

```
REST (page of 100):  Lib/distutils/tests/test_sysconfig.py  removed   changes 0, no patch
GraphQL files:       Lib/distutils/tests/test_sysconfig.py  DELETED   +0 -263
REST (page of 10):   Lib/distutils/tests/test_sysconfig.py  changes 263, patch present
```

Page 2 (13 files, 183 lines) had every patch. Re-requesting files 81–90 with `per_page=10&page=9` returned
every patch and the right counts. So the budget applies per response and is close to 20,000 changed lines:
19,822 fit, and the next file would have made 20,085 (inferred from these numbers; GitHub doesn't document
it). An OpenJDK Skara bug saw the same thing as "no `patch` past the 70th file when `per_page` exceeds 70" on
one large PR ([SKARA-1406](https://bugs.openjdk.org/browse/SKARA-1406)). A budget per response explains that
too, and a fixed count of 70 files can't explain the 86 here.

**Per file.** #138719 (Unicode 17.0, 11 files, +20,937 −20,874). Three generated tables have no `patch` but do
keep their counts, on a page of 100 and on a page of their own (`per_page=1`):

| File | Changed lines | Local diff size | Head blob | `patch` |
|---|---|---|---|---|
| `Objects/unicodetype_db.h` | 3,694 | 321 KB | 271,562 B | missing |
| `Modules/unicodedata_db.h` | 3,977 | 429 KB | 586,053 B | missing |
| `Modules/unicodename_db.h` | 34,081 | not measured | 1,432,688 B | missing |
| `Lib/distutils/dist.py` (#99061) | 1,256 | — | — | 51,659 B |

"Local diff size" is `git diff --no-index` over the base and head blobs, fetched with REST raw. Its stat,
`1782 insertions(+), 1912 deletions(-)`, matches GitHub's counts for `unicodetype_db.h` exactly. The cut isn't
caused by the `generated` attribute (`*_db.h` is `linguist-generated=true` in CPython's `.gitattributes`): a
20-line change to the generated `Modules/clinic/_tkinter.c.h` in #161368 has its patch. So the API leaves out
a file's patch somewhere between a 52 KB and a 321 KB diff, well under the 500 KB that github.com documents.
The exact threshold, and whether it counts bytes or lines, isn't settled.

**Binary.** #161214 renames `pip-26.2.2-py3-none-any.whl` to `pip-26.3-py3-none-any.whl` with new content:
`status: renamed`, `previous_filename` set, `changes: 0`, no `patch`. GraphQL reports the same file as
`RENAMED +0-0` and has no field for the old name.

So on a file entry with no `patch`:

| Counts | What it means | What to do |
|---|---|---|
| `changes: 0`, any status | dropped by the per-response budget, or a binary file | request it on a small page, e.g. `per_page=1&page=<index+1>` |
| `changes` > 0 | the file's diff is too large for the API | fetch both versions and diff locally |
| `changes: 0` again on a page of its own | binary (or an empty file, or a mode-only change) | show "binary", check `Blob.isBinary` if needed |

Only the second request tells a dropped file from a binary one: on the first page they look the same.

### REST `sha` and the two ends of the diff

Each entry's `sha` is the blob at the head (`9b9b429101…` for the wheel above, which matches the `.diff`'s
`index` line). The list carries no base blob id, so the base version is fetched by path: `previous_filename`
for a rename, otherwise `filename`.

GraphQL's `baseRefOid` and REST's `base.sha` are the base branch's commit as GitHub recorded it, which need
not be the merge base (inferred). GraphQL has no merge-base field (searched the schema). REST compare returns
it: `GET /compare/{baseRefOid}...{headRefOid}` for open PR #130771 gave `merge_base_commit` = `69e0a78e6e`
(equal to `baseRefOid` here, `behind_by: 0`, `ahead_by: 85`) and the same 6 files as the PR, each with its
`patch`. For a PR whose recorded base isn't an ancestor of its head, the merge base differs from `baseRefOid`
(inferred). The LEFT side of the diff, and a LEFT thread's `line`, are relative to the merge base.

### Whole files through GraphQL and REST

One GraphQL query with six `object(expression: "<oid>:<path>")` aliases over `... on Blob { oid byteSize
isTruncated isBinary text }` **cost 1 point** and returned 1.9 MB:

| Blob | `byteSize` | `isTruncated` | `text` returned |
|---|---|---|---|
| `Modules/unicodename_db.h` @ head | 1,432,688 | true | 512,000 bytes |
| `Modules/unicodedata_db.h` @ head | 586,053 | true | 512,000 bytes |
| `Modules/unicodedata_db.h` @ base | 588,046 | true | 512,000 bytes |
| `Objects/unicodetype_db.h` @ head | 271,562 | false | all |
| `Modules/unicodedata.c` @ head | 51,816 | false | all |
| a path that doesn't exist | — | — | `object` is `null`, no error |

So `text` stops at 512,000 bytes (500 KiB) exactly. REST `GET /contents/{path}?ref=<oid>` with `Accept:
application/vnd.github.raw+json` returned the 586 KB and 588 KB files whole, one request each. The GraphQL
point cost counts connections, and `object` isn't one, so a batch of blobs costs 1 point whatever its size
(inferred from the formula and this one query). Its time wasn't measured. Batching many large blobs could come
near the 10 s cut-off, so keep batches to a few MB.

## Cost of a large CPython PR

| Fetch | REST requests | GraphQL points | Note |
|---|---|---|---|
| File list of #99061 (113 files), `per_page=100` | 2 | — | 14 patches dropped |
| Re-request the 14 dropped files | 1–2 | — | `per_page=10` or by index |
| Same list through GraphQL `files(first: 100)` | — | 1 per page | counts only, no patches |
| Whole `.diff` | 1 | — | 406 past 20,000 lines |
| Merge base (compare) | 1 | — | also returns up to 300 files with patches |
| Both versions of one file | 2 (raw) | 1 per batch of files | fetched only to expand or when `patch` is missing |

A PR the size of #99061 costs about 5 REST requests up front. Even if a user expanded every file, the blobs
would cost a handful of GraphQL batches. Against 5,000 an hour, shared with `gh` (`research/github-sync.md`),
none of this matters. The real limits are the per-response line budget and the per-file cut. The 10 s timeout
didn't hit any call here: page 1 of #99061 returned 767 KB of patches without trouble.

Past 3,000 files the REST list stops, compare already stops at 300 files, and `.diff` stops at 300 files or
20,000 lines (from the docs, not tested). GraphQL `files` may list more, but it was not tested past 113 files.
None of the PRs looked at here came near 3,000 files. I didn't count the CPython PRs past 300 or 3,000 files.

## Review threads and the diff

Fields come from the [public schema][schema] and the [REST review comments
page](https://docs.github.com/en/rest/pulls/comments?apiVersion=2022-11-28), which describes request
parameters but not the response fields. Measured on #130771 (65 threads, 6 files): one GraphQL query, 1 point.

**`PullRequestReviewThread`**: `path`, `subjectType` (`LINE`, `FILE`), `diffSide` and `startDiffSide` (`LEFT`,
`RIGHT`), `line`, `startLine`, `originalLine`, `originalStartLine`, `isOutdated`, `isResolved`. Each comment
adds `commit`, `originalCommit` and `diffHunk`. `position`/`originalPosition` are deprecated: "Use the `line`
and `startLine` fields instead, which are file line numbers instead of diff line numbers".

The threads of #130771:

| Kind | Count | `line` | Comment's `commit` | Place it by |
|---|---|---|---|---|
| current, `RIGHT` | 18 | head line | = `headRefOid` | `line` in the head file |
| current, `LEFT` | 1 | 846 | = `headRefOid` | `line` in the merge-base file |
| outdated, `LINE` | 44 | `null` | an older commit | `originalLine` at `originalCommit` |
| outdated, `LINE`, range | 1 | 87 (start 81) | an older commit | as above; ignore `line` |
| outdated, `FILE` | 1 | 1 | an older commit | the file only |

From these threads:

- **`line` is in the current diff's coordinates.** On the `LEFT` thread, `originalLine` is 681, inside its
  `diffHunk`'s old range `@@ -669,29 +786,158 @@`, but `line` is 846. The old side shifted because the branch
  took in newer `main` (inferred from the numbers). `diffHunk` stays as it was when the thread was made.
  Anchor current threads by `line` and `diffSide`, never by `diffHunk`.
- **`startLine` equals `line` on single-line threads** (18 of the 19 current threads, with `originalStartLine`
  null). A thread spans several lines only when `startLine < line` or `startDiffSide` differs from `diffSide`.
  Only one current thread did (`startLine` 1134, `line` 1135).
- **`isOutdated` doesn't always mean `line: null`.** One outdated range still has `line` 87 and `startLine`
  81. Its comment's `commit` isn't the head, so the numbers belong to an older diff. Treat every outdated
  thread as anchored only by `originalLine`/`originalStartLine` at `originalCommit`.
- **A `FILE` thread has `line: 1` and `diffHunk: ""`.** Both are placeholders. Show it on the file, not on
  line 1.
- **Threads made on github.com on unchanged lines** come back with `diffHunk: ""` and lines outside every hunk
  (`research/expanded-line-comments.md`). The view has to expand those lines, or list such threads at the top
  of the file.
- Not tested: a thread on a renamed file. Inferred: `path` is the path in the commit the thread is anchored
  to, so a current thread uses the new name.

An outdated thread's `diffHunk` holds the lines above it, and its header gives their line numbers, so a short
view of it needs no fetch. A wider view needs its file at `originalCommit`: `object(expression:
"<originalCommit>:<path>")`.

## What this means for 0.2

1. **File list and patches: REST `GET /pulls/{n}/files?per_page=100`**, every page. It's the only source of
   patches per file, of the old path of a rename, and of the head blob id. A PR list with counts for the
   Workbench can keep using GraphQL `files`.
2. **Re-request dropped files.** For each entry with no `patch` and `changes: 0`, request it again on a small
   page (`per_page=1&page=<index+1>`, or a run of them with a small `per_page`). Never store the 0 counts from
   a page where patches were dropped.
3. **Diff locally when the API won't.** For a file with no `patch` but `changes` > 0, fetch the merge-base and
   head versions and run `git diff --no-index` (ADR 0003) or an equivalent. Mark the file "diffed by Tsugiki":
   GitHub may not accept line comments on lines its own diff doesn't show (#5), so the composer should allow
   only file-level comments there until we know.
4. **Expand context from whole files.** Fetch head and merge-base versions with GraphQL `object(expression:)`
   in batches. Refetch with REST raw when `isTruncated` is true (over 512,000 bytes). Get the merge base once
   per head commit from REST compare, and use `previous_filename` for the base path of a rename.
5. **Don't use `.diff` as the main source.** It's one request, but a PR past 20,000 lines gets nothing, and
   #99061 is already past it. It could be a fast path for small PRs. It doesn't seem worth having two parsers.
6. **Past 3,000 files (or 300 for compare) say so**, and wait for 0.3's mirror. Show the files GitHub listed,
   and a banner that the list is incomplete.
7. **Threads**: place current threads by `line`/`startLine` + `diffSide`/`startDiffSide` on the head or
   merge-base file. Treat `startLine == line` as a single line. Put outdated threads (and `FILE` threads) in a
   list per file, with their `diffHunk` as context.

## Open points

- The size at which the files list leaves out a single file's patch: between 52 KB and 321 KB of diff, bytes
  or lines unknown. Finding it would take a PR with one file in that range. It doesn't change the approach:
  handle a missing patch whatever the cause.
- The exact per-response budget: close to 20,000 changed lines, from one PR.
- Whether `.diff` gives 406 at 300 files and at 1 MB as the docs suggest; and whether GraphQL `files` lists
  more than 3,000 files.
- Whether a diff computed locally matches GitHub's hunks in general. The counts matched on one 3,694-line
  file. GitHub may use another diff algorithm or rename detection, and where hunks differ a line comment could
  get the 422 from #5.
- How long a batch of large blobs takes in GraphQL, against the 10 s limit.
- Thread `path` on renamed files.
