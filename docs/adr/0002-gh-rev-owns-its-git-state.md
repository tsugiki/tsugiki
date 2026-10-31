# gh-rev owns its git state; code comes from a local mirror, conversation comes from the API

Diffs, file contents, symbol lookup and staleness checks come from a bare mirror of `python/cpython` that gh-rev owns. It fetches `pull/N/head` refs into its own ref namespace. PRs, reviews, comments and CI status come from the GitHub API. Opening a file in an editor uses a single **Review checkout**, a worktree from the mirror that gh-rev switches to whichever PR is under review. gh-rev never reads from or writes to the user's own clone or worktrees, apart from an optional one-time bootstrap with `--reference` to avoid downloading the whole repository again.

The API alone is too slow, and it truncates large diffs, which CPython's Generated files regularly hit. Expanding context would cost an extra API call each time. Local git makes diffs instant and untruncated, works for offline reading, and makes the later interdiff essentially `git range-diff`. Keeping the mirror separate means gh-rev cannot disturb the user's branches, remotes or worktrees.

## Consequences

- Symbol lookup in v1 runs `git grep` against the PR's head commit (and its base commit for removed lines), behind a "definitions of X at commit C" interface. An LSP-backed implementation (clangd, pyright) can be added later behind the same interface. It was deferred because CPython on Windows has no `compile_commands.json`, needs a per-branch `pyconfig.h`, and clangd needs 1–2 GB of RAM plus a base-side index.
- Generated files are identified with `.gitattributes` at the PR's Target branch, read from the mirror.
