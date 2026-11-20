# The link rules are a set of named rules, parallel to the schema's migrations

The store keeps each PR's links as rows that the link rules work out when it saves the PR (#833): the
Issues the PR fixes, each under the Conventions it counts under, and the Origin PR it names. A PR that never
syncs again keeps its rows, so after a release changes the rules, the session works out every PR's links
again (#950). One number, `links::RULES_VERSION`, named the rules, and someone had to raise it by hand. Only a
pinned test made them, and it sampled a handful of PRs for all the rules together: a change to the closing
keywords that none of them reached passed with no raise (#843 §2).

## Decision

The rules are the variants of one enum, `LinkRule`, and `LinkRule::SET` holds the ones that run, as
`MIGRATIONS` holds the steps of the schema. The schema says what the store holds, and the rules say how
Tsugiki reads it. The first set holds five rules: GitHub's closing references, the closing keywords in the
description, CPython's Issue numbers in the title, CPython's Origin PR in the title, and the cherry-pick line.

- A released rule never changes what it gives, as a released migration never changes. A change to one is a
  new variant that takes its place in `SET`, and the old variant stays outside `SET` as a retired name. Every
  change to the rows counts: a new keyword, a new Conventions, or how a row spells its Conventions.
- Every link rule runs through `SET`: the store's `Keeper`, `fixed_under_each` and `origins_in_title`, and
  the in-memory reads the sync and the Workbench make (`fixed_numbers`, `possible_numbers`, `backport_of`,
  `possible_origin`). What each rule reads in a PR's text is private to `links/rules.rs`, so no code outside
  `LinkRule` can run it. The cherry-pick rule matches a commit to the PR that became it, which `MergeCommits`
  does among the facts and the `Keeper` does in SQL. The `Keeper` runs that SQL only when its rules hold the
  cherry-pick rule.
- The pinned test gives each rule its own sample PRs, and checks the rows that rule alone keeps for them,
  through a `Keeper` that runs only that rule. A second test checks that the whole set keeps the rows its
  rules keep alone, so the samples of each rule speak for the set. Its match on `LinkRule` has no default
  arm, so a new rule can't build without samples.
- `RULES_VERSION` stays until #971, which keeps the names of the rules in the store in its place. Until then
  a test pins it to `SET`, so a change to the set fails until someone raises it.

## Considered Options

- **Keep one number and widen the pinned test.** More samples reach more of each rule, but a change that no
  sample reaches still passes, and the number still says nothing about which rules wrote the rows. Two
  branches that each raise it to 2 would pass on each other's stores.
- **A version for each rule.** A number per rule is a name that only counts. A variant is a name the
  compiler checks, and #971 can keep it in the store.
- **Store the rule that wrote each row.** It would tell exactly which rows a retired rule left, but every
  row grows a column, and a refill already rewrites a PR's rows all at once.

## Consequences

- The CPython title parsers moved from `conventions.rs` to `links/rules.rs`, and `Conventions` lost
  `issues_in_title` and `origin_in_title`. A new Conventions brings rules of its own.
- A retired rule runs nowhere, so its arms can give nothing, and its samples can leave the pinned test.
- #971 keeps each rule's name in `link_rules`, refuses a store that names a rule this build doesn't know, and
  refills a store that lacks one. `RULES_VERSION` goes with it.
