---
name: wave
description: >-
  Build a milestone's tickets in waves, one background agent per ticket. Use when the user asks to plan the
  waves of a milestone, or to implement or run wave N.
---

A **wave** is a set of tickets, three so far, that background agents build at the same time, each in a
worktree of its own. A **lane** is a run of tickets that edit the same files, so they go one after another;
a wave takes at most one ticket from each lane. The tickets of one wave are **siblings**.

The milestone's description holds what belongs to that release: a `## Lanes` table, a `## Waves` table and a
`## Wave log`. This skill holds the procedure. The session that runs the wave is the only one that edits the
milestone's description.

- To plan the waves of a milestone, follow [plan.md](plan.md).
- To implement wave N, follow the steps below.

## Implement wave N

A wave runs for hours and outlives a `/clear`, so keep its record in the milestone's description, not in
the session: write each time and conflict into the wave's `## Wave log` entry as it happens. A session that
picks up a wave halfway reads the entry and goes on from there.

The wave's row in `## Waves` holds its tickets from the plan on, and the numbers that compare it with other
waves. Step 3 writes its Start, and step 8 the other cells from the entry:

- **Start**: when the agents start. **End**: the wave's last merge. Both in local time with the date, as
  `11-23 10:05`, since a wave can run past midnight.
- **Wall-clock**: End minus Start.
- **Slowest build**: the longest time an agent took from its start to reporting its build.
- **Slowest review**: the longest time from an agent reporting its build to its `just preflight` passing,
  review rounds and fixes included.
- **Rounds**: the most review rounds a ticket of the wave took.
- **Waiting on merge**: Wall-clock minus Slowest build minus Slowest review, which leaves CI and the time
  until the user merges.
- **Conflicts**: how many merges of main conflicted, with the hardest in brackets, as `2 (1 moderate)`, or
  `0`.

Read and write the description with `gh api repos/tsugiki/tsugiki/milestones/<number> --jq .description`
and `gh api -X PATCH repos/tsugiki/tsugiki/milestones/<number> -F description=@<file>`, from a file in
`.temp/scratchpad/`.

### 1. Check the wave

Take the tickets from wave N's row in `## Waves`. For each, check that it's open and list its open blockers
with `just tree --up <n>`.

A ticket blocked by a ticket outside the wave can't start. Offer the user a ready ticket from a later wave
for its **slot**, one that shares no lane with the other siblings. A ticket blocked by a sibling starts on a
branch off the sibling's, as [builder.md](builder.md) says; tell the user, since it takes about as long as
the two in a row.

For each ticket, read the PR body of each blocker that merged since the ticket was filed, its "What stays
open" part included, since a ticket filed before its blocker merged can ask for work the blocker did on the
way. `gh api repos/tsugiki/tsugiki/issues/<n>/dependencies/blocked_by` lists the blockers, closed ones too,
and `gh issue view <blocker> --json closedByPullRequestsReferences` names the PR. Where a blocker built part
of the ticket, or moved the code the ticket names, tell the user and fill in the "What a blocker built"
section of [builder.md](builder.md). A ticket its blockers finished goes back to the user, not to a builder.

Done when the user has agreed to the wave's tickets, each open and with no open blocker outside the wave,
and has heard what their merged blockers already built.

### 2. Make the worktrees

For each ticket, run `just worktree <n>`, or `just worktree <n> <slug>` for a shorter branch name than the
ticket's title gives. It prints the worktree's path and branch, which the builder's prompt takes.

A ticket that starts on a sibling's branch gets its worktree the same way, then merges that branch:
`git -C .temp/wt-<n> merge <sibling-branch>`. The worktrees share one repository, so the name is enough.

Add the wave's entry to `## Wave log`, from the form in step 8.

Done when each ticket has a worktree with `node_modules` installed, and the log has the wave's entry.

### 3. Start the agents

Start one background agent per ticket with the prompt in [builder.md](builder.md), its placeholders filled.
Start them in one message so they run at the same time, and write the time into the row's Start.

### 4. Review each build

Once an agent reports its build, write its build time into the entry, and run `/code-review` on its branch's
diff over `origin/main` in its worktree. Send the findings to that agent with SendMessage. It fixes each or
declines it with a reason; judge the reasons, and send back any you don't accept.

Review a build at most twice. The second round reviews only the fix commits, from the commit the first round
reviewed to the branch's head, before any merge of main, and asks for correctness: a bug a fix brought in,
or a finding a fix left half done. Name the findings already declined or kept in its prompt, so it doesn't
raise them again. Run a third round only when a fix added a mechanism of its own, such as a new table,
channel or type that other code goes through: in 0.3.2 a third round found a break only in such a mechanism,
and cleanups elsewhere.

When a round's findings share one cause, or a fix brings one in, ask the builder to change the cause once
and send no fix for each case: from 0.3.3 to 0.3.5, each such fix opened a new case. The causes so far,
with the change to ask for, and the rule in AGENTS.md it follows where there is one:

- Flags out of step: one value whose variants carry their data (make invalid states unrepresentable).
- Code that guesses which async answer belongs to which action: remove the guess.
- A second queue or scheduler beside one the code already has: fold its work into the existing one.
- An owner's data reaching another thread out of save order: the owner sends it (one writer, many readers).
- A ticket that only moves code, with bugs main has too: keep main's behaviour and file a ticket for them.
- Code working around an answer it didn't read in full: read it where it enters (parse at the boundary).

Two kinds of finding would grow a fix past its ticket, so hold each back from the builder:

- File a finding that asks for behaviour the ticket doesn't name as a ticket in the milestone being built,
  without asking the user, since a fix for behaviour no ticket specified brings findings of its own. Send the
  builder what the build gets wrong, or leaves out of what the ticket asks.
- Take a bug of the ticket's own whose cause lies in behaviour other features share to the user first, since
  they may want one rule for that behaviour, built in a ticket of its own. Then either the builder fixes only
  what the ticket adds, the way the code already works, and that ticket takes the cause, or the user widens
  the ticket.

Then tell it to run its pre-push gates, `just preflight`. Let one agent run them at a time, since they run
the full Vitest and Rust suites. `just preflight` runs no Playwright, so the builder also runs the e2e specs
that drive what it changed, as [builder.md](builder.md) says. Once they pass, write into the entry the
ticket's review time, from its build report to that pass, and its rounds.

Done when the agent reports the review fixed and its gates passed, and the entry has its review time.

### 5. Ship

From the worktree, run `just ship .temp/pr-body.md`. It takes the PR's title from the branch's first commit
and its body from the file the builder keeps. Report the PR and, once it finishes, CI's result to the user,
and write the result into the log. If CI fails, send the failed gate's log to the agent; it fixes the gate
and reruns only that gate, and you push again with `just ship`. A force push needs the user's say each time.

A PR that changes the gates ships with `just ship --check`, as AGENTS.md says. Start no agent's
`just preflight` while it runs, since its `just check` runs the same suites.

The user merges each PR. Stop at the report.

If a PR changes what `just smoke` covers (AGENTS.md names the files), ask the user before running it, and
run it only while no agent is building. First check that no Tsugiki is running: the single-instance lock
makes the run's Tsugiki exit at once with code 0, before any check.

### 6. After each merge

Once the user says a PR merged, tell each sibling still open to merge `origin/main` and rerun its tests, as
[builder.md](builder.md) says. Write into the entry one line for each sibling's merge of main: which files
conflicted, how hard each was, and each break the tests found with no conflict. Step 8 counts the lines with
a conflict for the row's Conflicts.

Once those tests pass, push with `just ship`, which runs `just quick` first: CI runs the full gates on the
push. Ask for `just preflight` again only when a
resolution rewrote code, a moderate or hard conflict, where a failed CI round trip costs more than the run.

### 7. Look for copies the siblings made

Once the wave's last PR has merged, bring the main checkout up to date with `git pull --ff-only --prune` and
run `just dupes` there. `just merged` can't do it: it wants a branch name, and the branch it would delete is
still held by the wave's worktree, which step 8 removes. Each PR passed against the main it started from, so a
helper two siblings each wrote shows up only once both are in. Fold each copy it finds into one place both
use, in one small PR, and wait for it to merge before the next wave starts. If a repeat has to stay, the PR
adds it to `ALLOWED` in `scripts/copies.ts` with why.

Done when `just dupes` passes on main.

### 8. Close the wave

The wave's entry in `## Wave log` keeps what its row leaves out, and takes this form:

```
### Wave N: #a, #b, #c

<A ticket that took another's slot, and why.>

Build and review, in minutes: #a A + R, #b B + S, #c C + T. <For each ticket, its review rounds and how
many findings each found; findings an agent declined.>

CI: <which PRs passed on the first push; each failure and its cause.>

Merge conflicts, merged in the order #x, #y, #z:

- #y after #x: <files>, none | easy | moderate | hard. <Breaks with no conflict.>

Follow-ups: <tickets filed from the reviews, and their milestone.>
```

After the last merge:

1. Fill in the follow-ups, and cut the parts of the form the wave had nothing for.
2. Fill in the row's End, then its other cells from the entry, as the list under "Implement wave N" says.
3. Update the row's Tickets if a slot changed hands, and move a ticket the wave couldn't start to a later
   wave's row.
4. If the wave taught something that holds for the next one, add it to this skill in a PR, as a rule with
   its reason in one clause. The story of how the wave learned it stays in the wave log.
5. Remove each worktree with `just worktree-remove <n>`, which deletes its builds and, once its PR has
   merged, its local branch.

Done when the entry has every part of the form filled or cut, the row has every cell filled, and no
worktree of the wave is left.
