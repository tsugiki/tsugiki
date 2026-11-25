# The builder's prompt

Fill the placeholders and give each agent this prompt. Leave out the "What a blocker built" section unless
step 1 of [SKILL.md](SKILL.md) found a merged blocker that built part of the ticket or moved its code, and
the "Branch off a sibling" section unless the ticket starts on a sibling's branch.

---

You build ticket #<n>, "<title>", in the worktree `<path>` on branch `<branch>`. Siblings build
tickets #<a> and #<b> in other worktrees at the same time; leave their worktrees and the main checkout alone.

Read `AGENTS.md`, the ticket (`gh issue view <n> --comments`) and the `docs/devel/` file of each `area:*`
label it carries. Follow `AGENTS.md`, with these changes for a wave:

- Run every command inside your worktree, with `CARGO_TARGET_DIR` unset, so Cargo builds into the
  worktree's own `target/`.
- Run a crate's tests through `just test-crate <crate>`, which fetches the language servers some tests
  start: without them those tests fail, and Cargo stops before the crate's integration tests.
- Put scratch files (probe scripts, copies of data, logs, message drafts) in your worktree's
  `.temp/scratchpad/`, not the scratchpad folder your system prompt names. Leave them there:
  `just worktree-remove` deletes them with the worktree, and the permission settings refuse your delete.
- Skip `just smoke` and the full Playwright suite. Run the spec files in `ui/e2e/` that drive what you
  changed, in both projects: `just e2e e2e/<file>.spec.ts`, whose path is relative to `ui/`. Grep them for
  the keys, commands and components your branch touches. If the page calls a command it didn't call before,
  run every spec: `ui/e2e/mock-core.ts` rejects a command a test doesn't answer.
- Before you report the build, run the tests of the files and crates you changed and the e2e specs that drive
  them. Leave the full Vitest, the coverage gates and Clippy across the workspace to `just preflight`: it runs
  them after the review, so a run before it is spent twice.
- Run tests in the foreground with a `timeout`, not in the background: a background run that ends can leave
  you waiting on it with nothing running.
- Allow 0.5px when you compare where an element sits on the screen: macOS's WebKit puts it a fraction of a
  pixel off.
- Read each item you put behind `#[cfg(unix)]` or `cfg(not(windows))` against the lints in `Cargo.toml` and
  `clippy.toml`, such as a `bool` parameter: Windows clippy never compiles it, so only macOS's CI would fail.
- To show that a test catches the bug, make it fail by taking out the fix itself, then put the fix back.
  Don't break the setup around the fix, such as a settings key the code reads: the test then fails for
  that reason alone.
- Commit to your branch. The session that started you runs the review and ships the PR, so don't push.
- `just ship` takes the PR's title from your branch's first commit and its body from your worktree's
  `.temp/pr-body.md`. Write that commit's message as the PR: a title that says what the ticket delivers,
  and a body that says why and how, with `Closes #<n>.` as its last line. Copy the body into
  `.temp/pr-body.md`.

Once the build is done, report what you changed, the files you touched and the tests you ran. The session
sends you `/code-review` findings. Fix each, or decline it with the reason it doesn't hold.

Put each round of fixes in a new commit, and leave the first one alone, so the session can review a round's
fixes on their own. Reread `.temp/pr-body.md` after each round, and update it where a fix made it wrong.

### The pre-push gates

Run `just preflight` once the session says to; it lets one agent run it at a time. Fix the gate that fails
and rerun `just preflight` until it passes.

Then read your diff for what only macOS's CI catches, as `AGENTS.md` says.

### After a sibling merges

After the session says a sibling merged, run `git fetch origin`, then `git merge origin/main`. Fix the
conflicts, then rerun `just vitest` and `just test-crate <crate>` for each crate either branch touched.
A merge with no conflict can still break: a function's new signature, a new argument, a test that mocks a
command's old answer. If both branches changed the generated bindings, run `just bindings`. Report the
conflicts and each break you fixed. Don't rerun `just preflight` unless the session asks: CI runs the gates on
the push.

### What a blocker built

#<blocker> merged as #<blocker-pr> after this ticket was filed, and <what it built of the ticket, or where it
moved the code the ticket names>. Read its PR body, `gh pr view <blocker-pr>`, before you start. Build what
the ticket still lacks, and say in your PR body what #<blocker-pr> already did.

### Branch off a sibling

Your branch starts from #<sibling>'s branch, `<sibling-branch>`, before that PR has merged. The worktrees
share one repository, so you can merge that branch by name. Merge its new commits as its review fixes come
in. Your PR waits for its merge.

Once it has merged, main holds it squashed, so a plain merge of main conflicts in every file both branches
touched. First merge the commit the PR merged from, `gh pr view <sibling-pr> --json headRefOid --jq
.headRefOid`, then `origin/main`: git then finds nothing left to merge in the files the sibling changed.
