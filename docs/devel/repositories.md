# Repositories

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Any number of Repositories, worked in one at a time: `python/cpython` and the user's own projects. Each has
  its own Workbench, Filters and Pinned issues. Per-repository conventions (CPython's `gh-NNNNN:` title
  prefix, `[3.x] … (GH-NNNNN)` Backports and `miss-islington`; elsewhere GitHub's "Fixes #N" links) are kept
  in one place: the Repository's **Conventions**, a preset whose fields can be edited ([ADR
  0011](../adr/0011-pr-links-from-github-signals-by-repository-conventions.md)).
- A **Home page** lists the Repositories, each with how many items Need attention there and what they are
  (Review requests, Stale reviews, failed CI on Own PRs, New activity, Dependency updates ready to merge) and
  when it was last opened, plus **Add repository**. Add repository on a Repository Tsugiki has already says
  so and offers **Switch to it**, which keeps its mirror answer. With no Repositories yet it shows onboarding
  instead: the token (gh's, or a read-only one made from the Account page's guide), the first Repository, an
  existing clone to borrow objects from. A normal launch reopens the last Repository; the Home page is one
  step away from the Repository switcher.
