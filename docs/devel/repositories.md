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
- The Home page lists the Repositories in the Repository picker's order, most recently opened first, so the
  top row is the Repository the user came from, and the rows keep their order while the page shows. Each row's
  first line holds `owner/name`, and on the right "opened 2 h ago", from `recent` in `machine.toml`, or
  nothing for a Repository never opened on this machine. ↑ ↓ move the cursor, and Enter or a click opens the
  Repository on its Workbench. A switch that finds its Repository gone from the files drops its row, and the
  cursor stays where the row was. On the Home page the command palette offers Repository: …, Add repository…
  and Settings, and has no quick open, which searches one Repository. The status bar shows there too.
