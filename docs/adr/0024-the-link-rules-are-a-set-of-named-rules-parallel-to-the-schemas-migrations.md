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
  `possible_origin`). `LinkRule::reading` names, for each rule, the one kind of link it gives and the
  function that reads it, and those functions are private to `links/rules.rs`, so no code reaches them
  except through `LinkRule`. The cherry-pick rule matches a commit to the PR that became it, which
  `MergeCommits` does among the facts and the `Keeper` does in SQL. Both run only when a rule of the set reads
  cherry-pick lines, and how each finds the PR belongs to that rule: a change to either is a new rule.
- The pinned test gives each rule its own sample PRs, and checks the rows that rule alone keeps for them,
  through a `Keeper` that runs only that rule. A second test checks that the whole set keeps the rows its
  rules keep alone, so the samples of each rule speak for the set. Its match on `LinkRule` has no default
  arm, so a new rule can't build without samples.
- The store keeps the name of each rule that wrote its rows in `link_rules`, one row each, in place of
  `RULES_VERSION` (#971). A migration turned version 1 into the first set's names. A store that names a rule
  this build doesn't know was filled by a newer Tsugiki, and it refuses to open with `NEWER`, as on a schema
  past the last migration it knows, so the older build doesn't rewrite the newer one's rows. A retired name
  still parses. A store whose names aren't `SET`'s holds stale links: one that lacks a rule of the set lacks
  its rows, and one that names a retired rule holds rows the set no longer gives. The session refills those
  links as a job (#950), then saves the set's names.

## Considered Options

- **Keep one number and widen the pinned test.** More samples reach more of each rule, but a change that no
  sample reaches still passes, and the number still says nothing about which rules wrote the rows. Two
  branches that each raise it to 2 would pass on each other's stores.
- **A version for each rule.** A number per rule is a name that only counts. A variant is a name the
  compiler checks, and #971 can keep it in the store.
- **Store the rule that wrote each row.** It would tell which rows a retired rule left, but every
  row grows a column, and a refill already rewrites a PR's rows all at once.

## Consequences

- The CPython title parsers moved from `conventions.rs` to `links/rules.rs`, and `Conventions` lost
  `issues_in_title` and `origin_in_title`. A new Conventions brings rules of its own.
- A retired rule runs nowhere, so its samples can leave the pinned test.
- Two cherry-pick rules can't both run, since the `Keeper` and `MergeCommits` each hold one way to find the
  PR. A new one replaces the old in `SET`, and the code of both looks at which rule the set holds.
- Each rule's name, as `link_rules` spells it, never changes once released, and a retired variant keeps its
  spelling. A downgrade to a build that lacks a rule the newer one ran won't open the store, and the user
  updates Tsugiki to read it.
- Two branches that each add a rule add two names, so a store that one of them filled names a rule the
  other doesn't know, and the other refuses to open it, where one number passed as current on both.
- A set that only drops a rule names nothing an older build doesn't know. The older build opens that store,
  finds a rule of its set missing, and refills, and the newer build refills again on its next start. A
  change that keeps to the rule above, a new variant in the old one's place, adds a name and avoids this.
