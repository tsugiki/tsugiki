# The builder's prompt

Fill the placeholders and give each agent this prompt. Leave out the "Branch off a sibling" section unless
the ticket starts on a sibling's branch.

---

You build ticket #<n>, "<title>", in the worktree `<path>` on branch `<branch>`. Siblings build
tickets #<a> and #<b> in other worktrees at the same time; leave their worktrees and the main checkout alone.

Read `AGENTS.md`, the ticket (`gh issue view <n> --comments`) and the `docs/devel/` file of each `area:*`
label it carries. Follow `AGENTS.md`, with these changes for a wave:

- Run every command inside your worktree. Build with `CARGO_TARGET_DIR=<path>/.temp/target` and
  `CARGO_BUILD_JOBS=4`, and run `rust-coverage` with `CARGO_TARGET_DIR=<path>/.temp/target-cov`.
- Skip `just smoke` and the Playwright tests.
- Commit to your branch. The session that started you runs the review and ships the PR, so don't push.

Once the build is done, report what you changed, the files you touched and the tests you ran. The session
sends you `/code-review` findings. Fix each, or decline it with the reason it doesn't hold.

### Before you report the review fixed

Run, one at a time, and fix what fails:

1. `cargo fmt --all --check`
2. `npm run vitest`, the full suite: CI's coverage gate wants 100% across the whole of it.
3. `npm run rust-coverage` with your coverage target.
4. `just typecheck` and `just doc-width`.

Then read your diff for what only macOS's CI catches, as `AGENTS.md` says.

### After a sibling merges

After the session says a sibling merged, run `git fetch origin`, then `git merge origin/main`. Fix the
conflicts, then rerun the full `npm run vitest` and the tests of each crate either branch touched. A merge
with no conflict can still break: a function's new signature, a new argument, a test that mocks a command's
old answer. If both branches changed the generated bindings, run `just bindings`. Report the conflicts and
each break you fixed.

### Branch off a sibling

Your branch starts from #<sibling>'s branch, `<sibling-branch>`, before that PR has merged. Merge its new
commits as its review fixes come in. Your PR waits for its merge. Once it has merged, main holds it squashed,
so a plain merge of main conflicts in every file both branches touched. Merge the sibling's last commit
first, then `origin/main`.
