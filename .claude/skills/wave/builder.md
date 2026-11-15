# The builder's prompt

Fill the placeholders and give each agent this prompt. Leave out the "Branch off a sibling" section unless
the ticket starts on a sibling's branch.

---

You build ticket #<n>, "<title>", in the worktree `<path>` on branch `<branch>`. Siblings build
tickets #<a> and #<b> in other worktrees at the same time; leave their worktrees and the main checkout alone.

Read `AGENTS.md`, the ticket (`gh issue view <n> --comments`) and the `docs/devel/` file of each `area:*`
label it carries. Follow `AGENTS.md`, with these changes for a wave:

- Run every command inside your worktree, with `CARGO_TARGET_DIR` unset, so Cargo builds into the
  worktree's own `target/`.
- Skip `just smoke` and the Playwright tests.
- Commit to your branch. The session that started you runs the review and ships the PR, so don't push.
- `just ship` takes the PR's title and body from your branch's first commit. Write that commit's message as
  the PR: a title that says what the ticket delivers, and a body that says why and how, with
  `Closes #<n>.` as its last line.

Once the build is done, report what you changed, the files you touched and the tests you ran. The session
sends you `/code-review` findings. Fix each, or decline it with the reason it doesn't hold.

### The pre-push gates

Run `just preflight` once the session says to; it lets one agent run it at a time. It runs the full Vitest
and Rust suites, since CI's coverage gates want 100% across the whole of each, and stops at the first gate
that fails. Fix it and rerun `just preflight` until it passes.

Then read your diff for what only macOS's CI catches, as `AGENTS.md` says.

### After a sibling merges

After the session says a sibling merged, run `git fetch origin`, then `git merge origin/main`. Fix the
conflicts, then rerun the full `npm run vitest` and the tests of each crate either branch touched. A merge
with no conflict can still break: a function's new signature, a new argument, a test that mocks a command's
old answer. If both branches changed the generated bindings, run `just bindings`. Report the conflicts and
each break you fixed.

### Branch off a sibling

Your branch starts from #<sibling>'s branch, `<sibling-branch>`, before that PR has merged. The worktrees
share one repository, so you can merge that branch by name. Merge its new commits as its review fixes come
in. Your PR waits for its merge.

Once it has merged, main holds it squashed, so a plain merge of main conflicts in every file both branches
touched. First merge the commit the PR merged from, `gh pr view <sibling-pr> --json headRefOid --jq
.headRefOid`, then `origin/main`: git then finds nothing left to merge in the files the sibling changed.
