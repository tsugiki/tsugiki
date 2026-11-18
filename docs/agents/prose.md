# Prose

Each kind of statement has one file that owns it. Route a statement to its file before you run `/stop-slop`
on it: that fixes how a sentence reads, and a sentence in the wrong file stays wrong however well it reads.
Write the trimmed version first, because review cuts a draft that defends itself.

## Who owns what

| Statement | Home | Keep it out of |
|---|---|---|
| What a word means | `CONTEXT.md` | a definition of its own in another file |
| What v1 does, and what it leaves out | `docs/scope.md` | how it behaves |
| How an area behaves | its file in `docs/devel/` | ADRs and comments |
| The order v1 is built in | `docs/build-order.md` | `docs/scope.md` |
| Why the design has this shape | `docs/adr/` | `docs/devel/`, comments, PR text |
| Where a module lives, which thread talks to which | `docs/architecture.md` | function names |
| A cost measured on one machine: a build size, a time | `docs/build-costs.md`, or an ADR's budget | comments, and docs that aren't about measuring |
| How to build, test and release | `CONTRIBUTING.md`, `docs/releasing.md`, the `justfile` | ADRs |
| How an agent works here | `AGENTS.md` and `docs/agents/` | `CONTRIBUTING.md` |
| A change a user can see | `CHANGELOG.md` | the mechanism behind it |
| What changed, what wasn't tested, and its limits | the PR's text | a lasting decision, which goes in an ADR |
| What one commit does | its message | the story of the branch |
| Work to do, or a question to decide | a GitHub issue | docs and comments |
| What the code can't say itself: a reason, an order not visible locally | a comment or docstring | a retelling of the code below it |
| A constraint that must not regress | a test's name and its assertion | a comment asserting it |
| What a file used to say | git | a dated note in the file |

## What to cut

- A reading from one run: a time, a size, a count of requests. It dates the text to one machine and one day.
- The story of how we got there: what was searched, what broke, what was tried. Keep the decision; the PR or
  the issue holds the rest.
- A comment that repeats what a test enforces, and the value the test asserts. The test fails when the rule
  breaks, and the comment goes stale.
- A count of the list beside it. A reader can check "the Workbench and the Inbox", and "two views" drifts
  when a third joins.

Keep the reason a claim gives. `AGENTS.md` gives one for each design rule, `#[expect]` requires one, and a
`Result` dropped with `let _ =` needs one.

Before you publish, check each count, ratio and directional claim ("faster", "fewer", "only") against the
code. When a PR moves code an ADR names, re-read the ADR and fix it in that PR.

## What stays

- A container the reader expects: an ADR's sections, the CHANGELOG's headings, a template's fields.
- A table or a list where the facts are enumerable.
- A short answer. Terse is the house style.
- A habit the file already keeps. Edit toward the voice in front of you.

## Checking an edit

Read once, at reading speed, what a trimming pass rewrote. If you backtrack, rewrite it. Strike each word the
pass added: if the sentence still claims what it claimed, the word goes. Put back each phrase the pass
replaced: if the old wording was sound and said the same, it wins. A shorter version that dropped a claim
doesn't win on length. A repair stays, such as the subject a split sentence needs.
