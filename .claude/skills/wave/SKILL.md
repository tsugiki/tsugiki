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
`just tree <n>`.

A ticket blocked by a ticket outside the wave can't start. Offer the user a ready ticket from a later wave
for its **slot**, one that shares no lane with the other siblings. A ticket blocked by a sibling starts on a
branch off the sibling's, as [builder.md](builder.md) says; tell the user, since it takes about as long as
the two in a row.

Done when the user has agreed to the wave's tickets, each open and with no open blocker outside the wave.

### 2. Make the worktrees

For each ticket, run `just worktree <n>`, or `just worktree <n> <slug>` for a shorter branch name than the
ticket's title gives. It prints the worktree's path and branch, which the builder's prompt takes.

A ticket that starts on a sibling's branch gets its worktree the same way, then merges that branch:
`git -C .temp/wt-<n> merge <sibling-branch>`. The worktrees share one repository, so the name is enough.

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
channel or type that other code goes through: in 0.3.2 a third round found a break only in such a mechanism,
and cleanups elsewhere.

If a round finds flags that fall out of step with each other, ask the builder to replace them with one value
whose variants carry their data, as AGENTS.md's "Make invalid states unrepresentable" says, and send no fix
for each case: each such fix lets the flags fall out of step some new way.

In the same way, when a round's findings come from code that guesses which async answer belongs to which
action, ask the builder to remove the guess, not to patch each race: in 0.3.3 each fix to such a guess
opened a new race.

Then tell it to run its pre-push gates, `just preflight`. Let one agent run them at a time, since they run
the full Vitest and Rust suites. `just preflight` runs no Playwright, so the builder also runs the e2e specs
that drive what it changed, as [builder.md](builder.md) says.

Done when the agent reports the review fixed and its gates passed.

### 5. Ship

From the worktree, run `just ship .temp/pr-body.md`. It takes the PR's title from the branch's first commit
and its body from the file the builder keeps. Report the PR and, once it finishes, CI's result to the user,
and write the result into the log. If CI fails, send the failed gate's log to the agent; it fixes the gate
and reruns only that gate, and you push again. A force push needs the user's say each time.

The user merges each PR. Stop at the report.

If a PR changes what `just smoke` covers (AGENTS.md names the files), ask the user before running it, and
run it only while no agent is building.

### 6. After each merge

Once the user says a PR merged, tell each sibling still open to merge `origin/main` and rerun its tests, as
[builder.md](builder.md) says. Write into the log which files conflicted, how hard each was, and each break
the tests found with no conflict.

Once those tests pass, push: CI runs the full gates on the push. Ask for `just preflight` again only when a
resolution rewrote code, a moderate or hard conflict, where a failed CI round trip costs more than the run.

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
3. If the wave taught something that holds for the next one, add it to this skill in a PR, as a rule with
   its reason in one clause. The story of how the wave learned it stays in the wave log.
4. Remove each worktree with `just worktree-remove <n>`, which deletes its builds and, once its PR has
   merged, its local branch.

Done when the entry has every part of the form filled or cut, and no worktree of the wave is left.
