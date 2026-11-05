# PR links come from GitHub's signals, read by each Repository's conventions

Which Issues a PR fixes, and which Origin PR a Backport comes from, are computed from what GitHub says now,
read through the Repository's **Conventions**. The links are not stored; like the other rules ([ADR
0010](0010-one-sqlite-database-and-per-repository-mirrors.md)), they are worked out when a screen asks, so a
retitled PR moves at the next sync with nothing to fix up.

A Repository's Conventions are a few named fields, filled in by a preset:

| Field | CPython | GitHub |
|---|---|---|
| Issue prefix in PR titles | `gh-NNNNN:` | none |
| Backport marker in titles | `(GH-NNNNN)` | none (no Backports) |
| Backport bots | `miss-islington` | none |
| GitHub's closing references | used | used |

`python/cpython` gets CPython when it is added, and every other Repository gets GitHub. The fields can be
edited on the Repository's Settings page, and an edited preset shows as Custom.

## Rules

- **PR to Issue.** A PR fixes every Issue named by any of: the title prefix, GitHub's closing references, and
  closing keywords (Fixes, Closes, Resolves and their forms, with `#N`, `GH-N` or the Issue's URL) in the
  description of a PR GitHub ignores them on, that is a Stacked PR or a Backport. All are added together;
  there is no order of trust. Measured on CPython, the title prefix is on 93 of 100 PRs, and where closing
  references exist the title's Issue was among them in 23 of 23. Branch names and bare mentions ("see gh-123")
  never link. A link to another repository, or to a number that is a PR rather than an Issue, is ignored. A PR
  left with no link is a **Standalone PR**.
- **Backport to Origin PR.** A PR is a Backport when it targets a branch other than the default branch and its
  title has the Backport marker. When the title lacks it, the `(cherry picked from commit <sha>)` line in its
  description or commits is matched to the merged PR whose merge commit that is. A Backport sits under its
  Origin PR in each Issue it is linked to by the rule above; CPython's Backport titles keep the `gh-NNNNN`
  prefix.
- **Bot or Manual backport.** A Backport is a Bot backport when a backport bot opened it and every non-merge
  commit in it was committed by that bot. A merge of the Target branch, as GitHub's Update branch button
  makes, doesn't count. A commit anyone else pushed, including an amended or force-pushed cherry-pick, makes
  it a Manual backport from then on.
- **Whole trees.** For every Issue the user's work reaches (a Filter match, a Pinned issue, or an Issue named
  by one of the user's PRs), Tsugiki reads the Issue's cross-reference events, newest first and paginated,
  fetches the PRs listed there that it doesn't have, and keeps those the rules above tie to the Issue. That
  drops PRs that only mention it. A Backport's Origin PR is fetched by number when missing. It goes one level:
  a PR in a tree that names another Issue doesn't bring that Issue's tree. Merged and closed PRs are fetched
  once and frozen, apart from a refresh that sees one move ([ADR
  0015](0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)).
- **Issues only the window reaches.** These show the PRs Tsugiki already holds, so a tree can lack PRs older
  than the window. [#129](https://github.com/tsugiki/tsugiki/issues/129) reads their trees too.

## Considered Options

- **Presets only**, needing a release for a new convention, or **free regular expressions** per Repository,
  which are easy to break and hard to read in Settings.
- **The title first, other signals only as a fallback.** It would lose a CPython PR's second Issue named by
  "Fixes", and the signals agreed whenever both were present.
- **Issue numbers in branch names** (`gh-12345-fix-foo`). They would catch a few more PRs, with wrong links
  among them.
- **Only the title marker for Backports**, which would leave a backport titled by hand as an ordinary PR on a
  maintenance branch. **Any PR on a maintenance branch is a Backport** would make Backports with no known
  Origin PR.
- **A Bot backport has at most one commit**, as the prototype had. Today's [3.14] backport
  python/cpython#161068 has an Update branch merge and would wrongly be reviewed on its own. **Matching the
  Origin PR's patch-ids** would turn many clean Bot backports Manual, because context lines differ on older
  branches. **Anything the bot opened** would let a person's conflict fix go unreviewed.
- **Local link overrides**, kept in Tsugiki like Local marks. They would mend a wrong title the user can't
  edit, but they add state that disagrees with GitHub; the fix belongs on GitHub. A **"moved to" note** on the
  Issue a PR left was also dropped, as GitHub shows none.
- **Trees limited to the synced window**, where a Pinned issue can look empty and a Backport can lose its
  Origin PR, or **completed only when the Issue view opens**, where rows and counts would change on opening.

## Consequences

- The sync stores what the rules need: PR titles, descriptions, Target branches, closing references, each
  commit's committer login and parent count, and merge commits. It doesn't store Issues' cross-reference
  events, because they only say which PRs to fetch and the links come from the PRs.
- A refresh reads cross-references only for an Issue that changed. A cross-reference doesn't always move an
  Issue's `updatedAt`, but a new link still arrives, because each refresh fetches each PR that changed.
- Measured on CPython with the starter Filters ([#103](https://github.com/tsugiki/tsugiki/issues/103)): a
  first sync sent 78 cross-reference queries out of 261, at one point each, and the two refreshes after it
  sent 1 and 0.
- A link made by hand in the Development panel may wait until its PR or Issue next changes. We couldn't check
  whether it moves the PR's `updatedAt`, and CPython had 1 such link in its latest 800 PRs.
- Changing a Repository's Conventions re-groups its Workbench at once, with no re-sync.
- A Bot backport that turns Manual starts needing its own review, which shows as a change of review state.
