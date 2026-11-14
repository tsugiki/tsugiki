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

### 1. Check the wave

Read the milestone's description (`gh api repos/tsugiki/tsugiki/milestones/<number> --jq .description`) and
take wave N's tickets. For each, check that it's open and list its open blockers with `just tree <n>`, or
with `just next` where that recipe doesn't exist yet.

A ticket blocked by a ticket outside the wave can't start. Offer the user a ready ticket from a later wave
for its **slot**, one that shares no lane with the other siblings. A ticket blocked by a sibling starts on a
branch off the sibling's, as [builder.md](builder.md) says; tell the user, since it takes about as long as
the two in a row.

Done when the user has agreed to the wave's tickets, each open and with no open blocker outside the wave.

### 2. Make the worktrees

For each ticket, from the main checkout:

```
git fetch origin
git worktree add .temp/wt-<n> -b <n>-<slug> origin/main
cd .temp/wt-<n> && npm ci
```

`<slug>` is a few words of the ticket's title, in the form our branches take. Each worktree builds into
targets of its own: `CARGO_TARGET_DIR=.temp/target` for the build and tests, and `.temp/target-cov` for
`rust-coverage`, both inside the worktree. A shared `target/` builds one worktree's crate against another
worktree's source.

Note the time. Done when each ticket has a worktree with `node_modules` installed.

### 3. Start the agents

Start one background agent per ticket with the prompt in [builder.md](builder.md), its placeholders filled.
Start them in one message so they run at the same time.

### 4. Review each build

Once an agent reports its build, note the time, and run `/code-review` on its branch's diff over
`origin/main` in its worktree. Send the findings to that agent with SendMessage. It fixes each or declines
it with a reason; judge the reasons, and send back any you don't accept.

Done when the agent reports the review fixed and the pre-push gates in [builder.md](builder.md) passed.

### 5. Ship

From the worktree, run `just ship`. Report the PR and, once it finishes, CI's result to the user. If CI
fails, send the failed gate's log to the agent; it fixes the gate and reruns only that gate, and you push
again. A force push needs the user's say each time.

The user merges each PR. Stop at the report.

If a PR changes what `just smoke` covers (AGENTS.md names the files), ask the user before running it, and
run it only while no agent is building.

### 6. After each merge

Once the user says a PR merged, tell each sibling still open to merge `origin/main` and rerun its tests, as
[builder.md](builder.md) says. Note which files conflicted, how hard each was, and each break the tests
found with no conflict.

### 7. Close the wave

After the last merge:

1. Add the wave to the milestone's `## Wave log`, in this form:

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

2. Update `## Waves` if a slot changed hands, and move a ticket the wave couldn't start to a later wave.
3. If the wave taught something that holds for the next one, add it to this skill in a PR.
4. Remove each worktree with `git worktree remove .temp/wt-<n>` once its PR has merged, and delete its local
   branch.

Done when the log entry has every part of the form filled from what you noted, and no worktree of the wave
is left.
