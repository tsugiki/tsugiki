# Plan the waves of a milestone

### 1. List the tickets and their blockers

List the milestone's open tickets with `gh issue list --milestone <title> --state open`. `just tree <n>...`
draws the open tickets below each one as a tree of blockers, with each one's depth. Until it exists, list
each ticket's open blockers with the `gh api .../dependencies/blocked_by` command in [SKILL.md](SKILL.md)'s
step 1. Note each ticket blocked from outside the milestone: no wave can start it, so it stays out of the
plan.

### 2. List the files each ticket edits

Read each ticket, its `area:*` files in `docs/devel/` and `docs/architecture.md`, and name the files you
expect the build to change. Mark the files several tickets share. In 0.3.1 these conflicted wave after wave:
a central UI store (`ui/src/lib/state/pr-files.svelte.ts`), `settings.svelte.ts`, `docs/scope.md` and the
`docs/devel/` file of the area.

Done when each ticket has a list of files.

### 3. Draw the lanes

Put tickets that share a file other than docs into one lane, ordered by their blockers. Two tickets that only
add to one `docs/devel/` file can go in different lanes: that conflict is easy. A lane follows files, not
features: two features that both edit one store are one lane.

### 4. Cut the waves

- Three tickets per wave, each from a different lane.
- No ticket in the same wave as a ticket it depends on. A chain inside one wave takes about as long as the
  chain in a row, and the squash merge makes the second branch conflict in every file both touched.
- Pair a big ticket, such as a new crate or a language server, with two that don't wait on it. The wave
  ends at its slowest PR.
- Put the ticket that edits the most shared files in the earliest wave it can go in, so later tickets build
  on it instead of merging it.

### 5. Write the plan

Show the user the lanes and waves. Once they agree, write them into the milestone's description:

```
## Lanes

Tickets in one lane edit the same files, so they go one after another. Tickets in different lanes can be
built at the same time.

| Lane | Order (→ blocks) |
|---|---|
| <name> | #a → #b; #c on its own |

## Waves

1. #a, #c, #e
2. ...

## Wave log
```

Write it as [SKILL.md](SKILL.md) says.
