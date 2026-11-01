# How a Repository's mirror is created and kept current

Research for [#13](https://github.com/tsugiki/tsugiki/issues/13). Context: [ADR 0002](../docs/adr/0002-tsugiki-owns-its-git-state.md) (Tsugiki owns a bare mirror, PR heads go into its own ref namespace, optional one-time `--reference` bootstrap), [ADR 0003](../docs/adr/0003-git-cli-over-libgit2-and-gitoxide.md) (git CLI), and the scope sections "Review state and activity" and "In-app review".

## Answer

Bootstrap with `git clone --bare --no-tags --reference <user's clone> --dissociate`: 36 s and 744 MB here, and the result does not depend on the user's clone afterwards. Without a clone to borrow from, use a plain full bare clone (3 min 37 s, 823 MB). Don't keep alternates: in the tests below the mirror broke when the source clone was moved, and again when the source pruned an object that the mirror had borrowed. Don't make `--filter=blob:none` the default either. It bootstraps in 32 s at 185 MB, but `git grep` at a commit fetches missing blobs one round trip at a time (about 1.2 s per blob), and offline reading only works for blobs fetched earlier. On each poll, compare the head SHAs from the API with the local refs and fetch only the PRs that changed, plus the Target branches, in a single `git fetch --stdin`. Delete the refs of PRs that leave the local cache with `git update-ref --stdin`. Tsugiki runs maintenance itself (not `git maintenance start`), and it treats a corrupt mirror as a cache: move it aside and re-clone.

## Setup

- Windows 11, Git 2.49.0.windows.1, home connection (about 6 MiB/s from GitHub in these runs). Clones in `%TEMP%`, deleted afterwards. Measured 2026-11-01.
- Source of truth: `https://github.com/python/cpython.git`. 2,306 open PRs at the time.
- The user's clone `X:\Work\cpython\main` was used only as a `--reference` source. Its `.git` and `objects/pack` listings (names, sizes, mtimes) were identical before and after. Its `upstream/main` was 2 weeks behind GitHub, so the `--reference` clone had to download 2 weeks of history.
- Every alternates breakage test used a copy of the mirror I made myself, never the user's clone.
- **macOS was not measured** (no Mac available to this session). Notes on macOS are inferences and are marked as such.
- All times are one wall-clock run (`time`) unless a range is given. Treat them as orders of magnitude.

## Bootstrap options (measured)

| Option | Wall time | Disk | Objects | Depends on the user's clone later? |
|---|---|---|---|---|
| `clone --bare --no-tags` (full) | 3 m 37 s | 823 MB (pack 818 MiB) | 1,099,312 | no |
| `clone --bare --no-tags --filter=blob:none` | 32 s | 185 MB (pack 182 MiB) | 746,022 | no |
| `clone --bare --no-tags --reference <clone>` (alternates kept) | 11.7 s | 43 MB of its own (pack 42 MiB, 18,888 objects) and it borrows the clone's 1 GiB | 18,888 local | **yes** |
| `clone --bare --no-tags --reference <clone> --dissociate` | 35.8 s | 744 MB (pack 734 MiB) | 1,099,312 | no |

- `--dissociate` gives a complete mirror that is smaller than a network clone (734 vs 818 MiB). The likely reason is that its local repack reuses the deltas of the user's well-packed clone (inference). After the clone, `objects/info/` holds no `alternates` file.
- A `--bare` clone fetches only `refs/heads/*` (6 branches here: `main`, `3.11`–`3.15`). Don't use `--mirror`: it maps `refs/*`, and GitHub advertises 83,386 `refs/pull/*` refs for CPython (`git ls-remote`, 3.5 s).
- The clone must be given the clone's common git dir, since a linked worktree has its own `.git` file. The prototype resolves it with `rev-parse --path-format=absolute --git-common-dir`; keep that. `--reference-if-able` skips a missing reference with a warning instead of failing ([git-clone](https://git-scm.com/docs/git-clone)).

### What happens to an alternates-based mirror (measured)

The git docs warn about this: if the source deletes branches and objects become unreferenced, "these objects may be removed by normal Git operations ... If these objects are removed and were referenced by the cloned repository, then the cloned repository will become corrupt" ([git-clone `--shared`, referenced from `--reference`](https://git-scm.com/docs/git-clone)). What I saw, with a copy I owned standing in for the user's clone:

1. **The source is moved.** Every command in the mirror fails: `error: unable to normalize alternate object path`, `fatal: bad object refs/heads/main`. A `git fetch` in that state downloaded a 665 MB pack, still failed (`did not send all necessary objects`), and left the pack behind. Moving the source back fixed it.
2. **The source drops a ref and is gc'd.** The source fetched a PR head into a branch. The mirror then fetched the same PR head and stored **0 objects** of its own, because the source already had them. The source deleted the branch and ran `reflog expire --expire=now --all` and `gc --prune=now`. In the mirror, `refs/tsugiki/pull/161082` became `fatal: bad object`, and `fsck` reported `invalid sha1 pointer`. This is the realistic case: the user runs `gh pr checkout` on a PR that Tsugiki also tracks, deletes the branch later, and the default `gc` prunes the objects 2 weeks after that ([gc.pruneExpire](https://git-scm.com/docs/git-gc)).
3. **`--dissociate`** copies the borrowed objects and removes the alternates file ([git-clone `--dissociate`](https://git-scm.com/docs/git-clone)), so neither failure can happen.

The prototype's `scripts/fetch/git.mjs` writes `objects/info/alternates` and keeps it. The real app should dissociate: clone with `--dissociate`, or, if it keeps the init-then-fetch flow, run `git repack -a -d` and then delete the alternates file.

## Fetching PR heads (measured, full mirror)

| What | Time |
|---|---|
| 50 PR heads, one `git fetch --stdin` with 50 refspecs | 2.95 s |
| 1 PR head per `git fetch` (5 runs) | 2.12–2.24 s each, so about 110 s for 50 (extrapolated) |
| Same 50 again, nothing changed | 0.86 s |
| All 2,306 open PRs, one `git fetch --stdin` | 44.8 s; the mirror grew from 823 MB to 1.1 GB |
| Same 2,306 again (poll) | 2.9 s |
| All 2,306 into the blob:none mirror | 16.6 s; pack grew from 182 to 253 MiB |
| Delete 2,256 PR refs, one `git update-ref --stdin` | 1.5 s |

- Refspecs have the form `+refs/pull/N/head:refs/tsugiki/pull/N`. GitHub's `refs/pull/` namespace is read-only ([GitHub docs](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/checking-out-pull-requests-locally)). `--stdin` reads refspecs one per line ([git-fetch](https://git-scm.com/docs/git-fetch)), which avoids the Windows command-line length limit.
- **One missing source ref fails the whole fetch** (`fatal: couldn't find remote ref refs/heads/no-such-branch`, exit 128). The prototype works around this by retrying refs one at a time. A cheaper fix that I measured: write the refspec as a glob, `+refs/heads/x*:refs/heads/x*` or `+refs/pull/N/head*:refs/tsugiki/pull/N*`. A glob that matches nothing is not an error (exit 0, and the other PR in the same fetch was still fetched).
- `git fetch --prune` does not help with closed PRs. It prunes refs that are gone on the remote, as a function of the refspec ([git-fetch PRUNING](https://git-scm.com/docs/git-fetch)), but GitHub keeps `refs/pull/N/head` after a PR closes: the `ls-remote` above lists 83k of them. Tsugiki must delete them itself.
- Unchanged polls are cheap (about 1–3 s), but the API already reports each PR's head SHA. So the cheapest poll compares the SHAs with `git for-each-ref refs/tsugiki/pull` and makes no git network call at all when nothing changed (design inference).

## Reads the app needs: full vs blob:none (measured)

PR 161066 (11 files, including the Generated `configure`) and PR 160541 (9 files).

| Operation | Full mirror | blob:none, cold | blob:none, warm |
|---|---|---|---|
| `merge-base main <PR>` | not timed (no noticeable delay) | 0.04 s (commits and trees are local) | — |
| `diff --stat <merge-base> <PR>` (11 files) | 0.065 s | 1.88 s (one batched fetch) | 0.062 s |
| One file: `diff -- <file> \| patch-id --verbatim` | 0.043 s | 1.11 s | — |
| Whole-PR diff, split into per-file patch-ids in one `patch-id` call (9 files) | — | 1.25 s | — |
| `check-attr --source=refs/heads/main generated -- <3 paths>` | — | 1.18 s | 0.060 s |
| `show 3.13:.gitattributes` | — | 2.2 s | — |
| `grep -w <sym> <PR head>`, whole tree (5,793 files, 131 MiB) | 0.29 s | not run; only 63 files under `Include/cpython` took **78 s** (about 1.2 s per blob) and triggered an auto gc | 0.18 s after a batched prefetch |
| Batched prefetch of one tree's missing blobs (5,554 blobs) | — | 11.9 s, +43 MB | — |
| `worktree add` (Review checkout) | 3.8 s | 4.0 s | — |
| Switch the Review checkout to another PR | 0.51 s | 3.4 s | — |

What each operation needs locally:

- **Patch-id staleness** needs the commits and trees of both PR revisions and of the Target branch (for the merge-base), and the blobs of the changed files on both sides of each diff. On blob:none, one `git diff` fetches all the missing blobs it needs in a single batch, but every *separate* `git diff -- <file>` pays its own round trip. Tip, measured: run one `git diff` for the whole PR, put a `commit <id>` line before each `diff --git` header, and pipe the result to one `git patch-id --verbatim`. It prints one id per file, and the ids match the per-file ones ([git-patch-id](https://git-scm.com/docs/git-patch-id)).
- **`.gitattributes` at the Target branch** (`check-attr --source`, [git-check-attr](https://git-scm.com/docs/git-check-attr)) needs the `.gitattributes` blobs along each path. That is one or two lazy fetches cold, and nothing once they are local.
- **Expanded context** reads the PR-head blob of a file the diff touched. The diff already fetched it, so this costs nothing more on either option.
- **Symbol peek** (`git grep` at a commit, [git-grep](https://git-scm.com/docs/git-grep)) needs every blob in the commit's tree. This is where blob:none fails. The [partial clone design doc](https://git-scm.com/docs/partial-clone) says: "Dynamic object fetching invokes fetch-pack once for each item ... This may incur significant overhead". A batched prefetch works: list the missing blobs with `git rev-list --objects --no-walk --missing=print <commit>`, then fetch them by id with `git -c fetch.negotiationAlgorithm=noop fetch --no-tags --no-write-fetch-head --recurse-submodules=no --filter=blob:none --stdin origin`, which is what git's own lazy fetch runs. The same fetch without the `noop` negotiation and `--recurse-submodules=no` downloaded the data, then failed its connectivity check and threw the pack away. Once `main`'s tip tree is local, other PR heads are only a few hundred blobs away (535 missing for `main` after prefetching one PR tree).
- **The Review checkout** needs the full tree of the PR head. `checkout`/`worktree add` prefetch missing blobs in one batch ([partial-clone](https://git-scm.com/docs/partial-clone)), so this works on blob:none but pays a round trip on each switch.

Conclusion: blob:none saves about 640 MB of history blobs and 3 minutes at bootstrap. In exchange, Tsugiki has to own a prefetch layer (main's tip tree, both sides of each fetched PR, each Review checkout), offline reading becomes partial, and `grep` stays slow for any tree not yet prefetched. For a CPython core developer, who almost always has a clone to borrow from, `--reference --dissociate` gets a full mirror in 36 s. So blob:none is not worth it as the default. It could come back later as a "low disk / no local clone" setting.

## Maintenance and corruption (measured, full mirror, about 1 GiB)

| Operation | Time |
|---|---|
| `maintenance run --task=commit-graph --task=loose-objects --task=incremental-repack` | 6.4 s |
| `fsck --connectivity-only --no-dangling` | 7.3 s |
| `fsck --no-dangling` (full) | 2 m 04 s |
| `gc` after deleting 2,256 PR refs | 35.5 s, leaving an 853 MB pack and a 58 MB cruft pack (unreachable objects kept for 2 weeks) |
| `gc --prune=now` on the 734 MiB dissociated copy | 19.3 s |
| `gc` on the blob:none mirror | 7.4 s |

- An auto gc started by itself in the blob:none mirror (`Auto packing the repository...`) after its lazy fetches created many small packs (`gc.autoPackLimit` defaults to 50, [git-gc](https://git-scm.com/docs/git-gc)). Auto maintenance detaches by default ([git-maintenance](https://git-scm.com/docs/git-maintenance)), so it would run concurrently with Tsugiki's own git calls.
- `git maintenance start` and `register` add the repository to `maintenance.repo` in the user's **global** git config, and install an hourly job with `schtasks` on Windows or `launchd` on macOS ([git-maintenance](https://git-scm.com/docs/git-maintenance)). That writes outside Tsugiki's own state, so ADR 0002's intent rules it out.
- gc keeps every object reachable from `refs/*` ([git-gc NOTES](https://git-scm.com/docs/git-gc)). Keeping a ref for each reviewed commit is therefore enough to keep it after a force-push.

## Recommendation

**Bootstrap**

1. If the machine config names a clone for the Repository, resolve its common git dir and run
   `git clone --bare --no-tags --reference-if-able <common-dir> --dissociate https://github.com/<owner>/<repo>.git <mirror>`.
   (`--reference` was measured; `--reference-if-able` is the same option with a missing path tolerated.) Without a clone, or if that fails, run `git clone --bare --no-tags <url> <mirror>`, which takes minutes for CPython and needs a progress display.
2. Clone into `<mirror>.tmp`, then rename it, so a crash never leaves a half-built mirror in place.
3. In the mirror's config: `maintenance.auto=false` (no detached gc during reads), `core.commitGraph=true`, `fetch.writeCommitGraph=true`. Clone `--no-tags`; the app never needs tags.

**Refs**

- `refs/heads/<branch>` for each Target branch (`main`, `3.x`).
- `refs/tsugiki/pull/<N>` for PR heads.
- `refs/tsugiki/reviewed/<N>/<sha>` for each commit that a Review or Local mark was made against, so that staleness can always diff it, even after a force-push and a gc.

**Update on each poll**

1. From the API results, collect the PRs whose `headRefOid` differs from the local ref (one `git for-each-ref refs/tsugiki/`). If none differ and the Target branches are unchanged, skip git.
2. Otherwise run one `git fetch --no-tags --stdin origin`, with glob-form refspecs (`+refs/pull/N/head*:refs/tsugiki/pull/N*`, `+refs/heads/main*:refs/heads/main*`) so one vanished ref can't fail the batch. This takes about 1–3 s.
3. Fetch the PRs that the Workbench, pins and Reviews reach, plus any PR when it is opened, rather than all 2,306 open PRs (+200 MB, 45 s the first time). Fetching all would be affordable, but the scope doesn't need it.
4. If a reviewed commit is missing, try `git fetch origin <sha>`. Not measured: GitHub serving an unreferenced, force-pushed-away commit by SHA is likely but not guaranteed, so the reviewed refs above are the real safeguard.
5. Once a PR leaves the local cache (closed and no longer shown, pinned or reviewed), delete its refs with one `git update-ref --stdin` batch (1.5 s for 2,256 refs). The next gc reclaims the objects after the 2-week grace period.

**Maintenance** (Tsugiki schedules it, when idle, behind the same lock as its other git writes)

- Daily: `git maintenance run --task=commit-graph --task=loose-objects --task=incremental-repack --task=pack-refs` (about 6 s).
- Weekly, or after a large ref prune: `git gc` (about 35 s).
- Never `git maintenance start`/`register`.

**Corruption**

- Signs: git fails with `bad object`, `invalid sha1 pointer` or `unable to read`. Run `git fsck --connectivity-only` (7 s) to confirm.
- On failure, don't repair in place. Rename the mirror to `<mirror>.broken`, bootstrap again (36 s with a reference, about 3.5 min without), re-fetch the Target branches, the PR refs and the reviewed SHAs from the local database, then delete the broken copy.
- Offer the full `git fsck` (2 min) only as a manual "Verify mirror" action.

## macOS (inferred, not measured)

- The bootstrap is mostly network transfer plus `index-pack`, so times should be similar on the same connection.
- Local steps that write many files (worktree add, switch) are probably faster without Windows Defender scanning each file.
- The alternates failures are the same on any OS. `--dissociate` removes them on both.
- The macOS git that comes with Xcode Command Line Tools usually lags upstream. Tsugiki should check `git --version` at start-up; everything here was run with 2.49.
