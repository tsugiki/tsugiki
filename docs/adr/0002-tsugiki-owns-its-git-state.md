# Tsugiki owns its git state; code comes from a local mirror, conversation comes from the API

Diffs, file contents, symbol lookup and staleness checks come from a bare mirror of `python/cpython` that
Tsugiki owns. It fetches `pull/N/head` refs into its own ref namespace. PRs, reviews, comments and CI status
come from the GitHub API. Opening a file in an editor uses a single **Review checkout**, a worktree from the
mirror that Tsugiki switches to whichever PR is under review. Tsugiki never reads from or writes to the user's
own clone or worktrees, apart from an optional one-time bootstrap with `--reference` to avoid downloading the
whole repository again.

The API alone is too slow, and it truncates large diffs, which CPython's Generated files regularly hit.
Expanding context would cost an extra API call each time. Local git makes diffs instant and untruncated, works
for offline reading, and makes the later interdiff essentially `git range-diff`. Keeping the mirror separate
means Tsugiki cannot disturb the user's branches, remotes or worktrees.

## Considered Options

- **`gh pr checkout N`.** It is the quickest way for a person to run a PR on their machine, and Tsugiki
  doesn't use it. It makes a branch and checks it out, but the mirror is bare: the diff, `git grep` and the
  merge-base read commits, so a working tree adds nothing. Run in the user's clone, it would move their branch
  and working tree, which this ADR rules out. It asks GitHub's API about the PR with the user's token, and its
  fetch uses whatever credentials git has, while the mirror fetches a public Repository anonymously with no
  credential helper (ADR 0005). If the author allows maintainer edits, it points the branch at the
  contributor's fork, so a push lands there. Fetching `pull/N/head` asks the base repository alone, and no
  fork becomes a remote.

## Consequences

- Symbol lookup in v1 runs `git grep` against the PR's head commit (and its base commit for removed lines),
  behind a "definitions of X at commit C" interface. It looks for Python's `def`, `async def`, `class` and
  module-level `NAME =`; C in CPython's style (a line starting with `NAME(` under its return type, one-line
  definitions, `#define`, `struct`, `enum` and `typedef`); and reST directives in `Doc/` such as `..
  function::` and `.. c:function::`. Hits are ranked by the same file, the same directory, the same language,
  then the rest. For other languages, or with no hit, it falls back to a whole-word search, labelled as such.
  It stops at 2 seconds or 50 hits. An LSP-backed implementation (clangd, pyright) can be added later behind
  the same interface. It was deferred because CPython on Windows has no `compile_commands.json`, needs a
  per-branch `pyconfig.h`, and clangd needs 1–2 GB of RAM plus a base-side index.
- The **Review checkout** is made on the first Open in editor, as a detached worktree of the mirror in
  `repos\<owner>__<name>\checkout\`. Each Open in editor moves it to that PR's current head, with no branches.
  If it has changes, Tsugiki asks first: Discard and switch, Copy as patch (its `git diff` to the clipboard),
  or Cancel. It is removed with its Repository, or by Remove checkout in Settings.
- Until the mirror's first clone is done, symbol lookup and Open in editor wait for it. A PR head newer than
  the mirror is fetched before either runs.
- Generated files are identified with `.gitattributes` at the PR's Target branch, read from the mirror.
