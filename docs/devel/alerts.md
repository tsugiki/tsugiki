# Alerts

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- An **Alert** is raised for what only the user can unblock ([ADR
  0016](../adr/0016-alerts-launch-tsugiki-links-own-toasts-on-windows-the-plugin-on-macos.md)): CI finished on
  an Own PR, a Review request, a Review on an Own PR, and a failed Bot backport of the user's Origin PR. A
  reply or mention addressed to the user is a kind that starts off. Each kind can be switched off in Settings
  › General. None is raised while a Tsugiki window has focus, or for what a first sync finds; a Muted item
  raises only Review requests; more than 3 at once become one Alert that opens the Home page. A click opens
  the PR in its Repository. There is no list of past Alerts: every kind leaves the item Needing attention, at
  the top of the Workbench.
