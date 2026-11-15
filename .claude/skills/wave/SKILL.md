---
name: wave
description: >-
  Build a milestone's tickets in waves, one background agent per ticket. Use when the user asks to plan the
  waves of a milestone, or to implement or run wave N.
---

A **wave** is a set of tickets, three so far, that background agents build at the same time, each in a
worktree of its own. A **lane** is a run of tickets that edit the same files, so they go one after another;
a wave takes at most one ticket from each lane. The tickets of one wave are **siblings**.

The milestone's description holds what belongs to that release: a `## Lanes` table, a `## Waves` list and a
`## Wave log`. This skill holds the procedure. The session that runs the wave is the only one that edits the
milestone's description.

- To plan the waves of a milestone, follow [plan.md](plan.md).
- To implement wave N, follow the steps below.

## Implement wave N

A wave runs for hours and outlives a `/clear`, so keep its record in the milestone's `## Wave log`, not in
the session: write each time and conflict into the wave's entry as it happens. A session that picks up a
wave halfway reads the entry and goes on from there.

Read and write the description with `gh api repos/tsugiki/tsugiki/milestones/<number> --jq .description`
and `gh api -X PATCH repos/tsugiki/tsugiki/milestones/<number> -F description=@<file>`, from a file in
`.temp/scratchpad/`.

### 1. Check the wave

Take wave N's tickets from `## Waves`. For each, check that it's open and list its open blockers with
`just tree <n>`, or, where that recipe doesn't exist yet, with:

```
gh api repos/tsugiki/tsugiki/issues/<n>/dependencies/blocked_by --jq '.[] | select(.state == "open") | .number'
```

A ticket blocked by a ticket outside the wave can't start. Offer the user a ready ticket from a later wave
for its **slot**, one that shares no lane with the other siblings. A ticket blocked by a sibling starts on a
branch off the sibling's, as [builder.md](builder.md) says; tell the user, since it takes about as long as
the two in a row.

Done when the user has agreed to the wave's tickets, each open and with no open blocker outside the wave.

### 2. Make the worktrees

For each ticket, run `just worktree <n>`. It makes `.temp/wt-<n>` on a new branch off `origin/main`, runs
`npm ci` there and copies the main checkout's `target/tools`, so the Rust tests find ty and clangd without
fetching them. It prints the worktree's path and branch, which the builder's prompt takes. The branch's slug
comes from the ticket's title; name a shorter one with `just worktree <n> <slug>`.

A ticket that starts on a sibling's branch gets its worktree the same way, then merges that branch:
`git -C .temp/wt-<n> merge <sibling-branch>`. The worktrees share one repository, so the name is enough.

Each worktree builds into its own `target/`, Cargo's default there, and the recipe refuses to run while
`CARGO_TARGET_DIR` is set: a shared target builds one worktree's crate against another worktree's source.

Add the wave's entry to `## Wave log`, from the form in step 7, with the agents' start time.

Done when each ticket has a worktree with `node_modules` installed, and the log has the wave's entry.

### 3. Start the agents

Start one background agent per ticket with the prompt in [builder.md](builder.md), its placeholders filled.
Start them in one message so they run at the same time.

### 4. Review each build

Once an agent reports its build, write its build time into the log, and run `/code-review` on its branch's
diff over `origin/main` in its worktree. Send the findings to that agent with SendMessage. It fixes each or
declines it with a reason; judge the reasons, and send back any you don't accept.

Review a build at most twice. The second round reviews only the fix commits, from the commit the first round
reviewed to the branch's head, before any merge of main, and asks for correctness: a bug a fix brought in,
or a finding a fix left half done. Name the findings already declined or kept in its prompt, so it doesn't
raise them again. Run a third round only when a fix added a mechanism of its own, such as a new table,
channel or type that other code goes through. In wave 4 of 0.3.2 the second rounds found the bugs that
mattered, both in code a fix had added, and each third round found cleanups and cost 10 to 15 minutes. In
wave 5, #686's third round, run because round 2 had rebuilt round 1's mechanism, found a break the rebuild
brought in.

If a round finds flags that fall out of step with each other, ask the builder to replace them with one value
whose variants carry their data, and send no fix for each case. In wave 7 of 0.3.2, #757's
theme preview gained flags in rounds 1 and 2, and round 3 found four ways they fell out of step, such as a
window left on the wrong side. The builder made them one value (off, on, ending), and round 4 traced every
transition and found only narrow timing cases.

Then tell it to run its pre-push gates, `just preflight`. Let one agent run them at a time: each runs the
full vitest and `rust-coverage`, which AGENTS.md leaves to CI outside a wave. In a wave a CI round trip
costs more, and in 0.3.1 two of three first pushes failed coverage without them. `just preflight` runs no
Playwright, so the builder runs the specs that drive what it changed, as [builder.md](builder.md) says: in
wave 5 of 0.3.2, #557 changed what Ctrl+Shift+P opens with, and three e2e tests of the palette failed on CI.

Done when the agent reports the review fixed and its gates passed.

### 5. Ship

From the worktree, run `just ship`. It takes the PR's title and body from the branch's first commit. Report
the PR and, once it finishes, CI's result to the user, and write the result into the log. If CI fails, send
the failed gate's log to the agent; it fixes the gate and reruns only that gate, and you push again. A force
push needs the user's say each time.

The user merges each PR. Stop at the report.

If a PR changes what `just smoke` covers (AGENTS.md names the files), ask the user before running it, and
run it only while no agent is building.

### 6. After each merge

Once the user says a PR merged, tell each sibling still open to merge `origin/main` and rerun its tests, as
[builder.md](builder.md) says. Write into the log which files conflicted, how hard each was, and each break
the tests found with no conflict.

Once those tests pass, push: CI runs the full gates on the push. Ask for `just preflight` again only when a
resolution rewrote code, a moderate or hard conflict, where a failed CI round trip costs more than the run.
In wave 4 of 0.3.2 a second `just preflight` after a conflict only in `keyboard.md` held the last PR back 8
minutes.

### 7. Close the wave

The wave's entry in `## Wave log` takes this form:

```
### Wave N: #a, #b, #c (YYYY-MM-DD)

<A ticket that took another's slot, and why.>

H h MM min from the agents' start to the last merge (#PRs). The agents took about A (#a), B (#b) and
C (#c) minutes to build. <Review rounds and how many findings; findings an agent declined.>

CI: <which PRs passed on the first push; each failure and its cause.>

Merge conflicts, merged in the order #x, #y, #z:

- #y after #x: <files>, easy | moderate | hard. <Breaks with no conflict.>

Follow-ups: <tickets filed from the reviews, and their milestone.>
```

After the last merge:

1. Fill in the wall-clock time and the follow-ups, and cut the parts of the form the wave had nothing for.
2. Update `## Waves` if a slot changed hands, and move a ticket the wave couldn't start to a later wave.
3. If the wave taught something that holds for the next one, add it to this skill in a PR.
4. Remove each worktree with `just worktree-remove <n>`, which deletes its builds and, once its PR has
   merged, its local branch.

Done when the entry has every part of the form filled or cut, and no worktree of the wave is left.
