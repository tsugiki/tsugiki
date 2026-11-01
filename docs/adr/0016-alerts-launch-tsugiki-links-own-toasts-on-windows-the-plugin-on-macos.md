# Alerts launch `tsugiki://` links: own toasts on Windows, the plugin on macOS

An **Alert** is an OS notification for something only the user can unblock. Every Alert carries a link such as `tsugiki://pr/python/cpython/140124`, and a click on it opens that PR in its Repository. Both OSes hand the link to one handler in the core, so a `tsugiki://` link opened from a browser works too. The facts come from [Can Tauri raise Alerts that open the right PR when clicked?](https://github.com/tsugiki/tsugiki/issues/20) (branch `research/alerts`).

- **Windows.** Tsugiki builds its toasts itself on the `windows` crate, with AppUserModelID `io.github.tsugiki`. A toast's `launch` is the link, with `activationType="protocol"`, so a click makes Windows run `tsg` with it. `tsg` passes the link to the running instance, or starts Tsugiki ([ADR 0007](0007-core-library-and-thin-tauri-app.md)). This works from Notification Center too, and needs no COM activator. The toast's `Tag` is the PR and its `Group` the kind of Alert, so a newer Alert for a PR replaces the older one, and opening the PR in Tsugiki removes its toast. The per-user installer ([ADR 0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)) writes the Start menu shortcut with the AppUserModelID and registers the `tsugiki` scheme, through the deep-link plugin's `desktop.schemes`. Without the shortcut, Windows shows no toasts.
- **macOS.** `tauri-plugin-notification` is enough for v1. It needs no Developer ID signing, and the link comes back through its click callback while Tsugiki runs, from a banner or from Notification Center. In v1 an Alert can't replace an older one, and a click on an Alert after a restart opens nothing.
- **Development builds** use a `tsugiki-dev` scheme, so they don't take over the installed copy's links.

## Which Alerts, and when

- The kinds on by default are: CI finished on an **Own PR** (passed or failed), a **Review request** to the user, a **Review** submitted on an Own PR, and a failed **Bot backport** of the user's **Origin PR**. A reply or mention addressed to the user is a kind that starts off. Each kind is a switch in Settings › General. Plain **New activity**, other people's CI, and token or sync problems never raise an Alert; the last two are the status bar's.
- Alerts are held back while a Tsugiki window has focus, and for what a Repository's first sync finds. A **Muted** item raises only Review request Alerts; an **Archived** one alerts as usual. More than 3 at once, as after start-up or coming back online, become one Alert ("5 things need you in python/cpython") that opens the Home page. Nothing is raised while Tsugiki isn't running.
- An Alert's title says what happened: "CI failed", "Review requested", "Changes requested by vstinner", "Bot backport to 3.13 failed". The body gives the PR's title and number, the Repository, and for CI the failed checks. A click on a Review Alert opens the PR at that Review.

## No list of Alerts

A missed Alert is found in the Workbench. Every kind leaves an item that **Needs attention** until the user opens it: a Review request, or New activity on an Own PR or on its Origin PR. The Workbench, the Filters and Pinned issues list the items that Need attention first, newest first by when they started to need it. A row's preview uses the Alert's words, and the Home page counts everything that Needs attention in each Repository.

## Considered Options

- **The notification plugin on Windows too.** On Windows it reports a click only while the toast is on screen, so a click from Notification Center is lost. It also can't replace or remove a toast.
- **`UNUserNotificationCenter` on macOS now.** It removes the macOS limits, but it needs a permission prompt and a signed bundle, and sources disagree on whether ad-hoc signing is enough. It fits with Developer ID signing, which [ADR 0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md) puts off.
- **Fewer kinds**, only CI on Own PRs, or **more**, an Alert whenever anything starts to Need attention. The first misses Review requests; the second interrupts for what can wait in the Workbench.
- **Holding back only while focused**, so a burst comes as many Alerts, or **quiet hours**.
- **An Alerts view** listing recent Alerts, or **a bell** in the status bar across Repositories. Both repeat what the Workbench's order and the Home page's counts already show.

## Consequences

- The macOS release checklist ([ADR 0009](0009-testing-and-full-coverage.md)) gains three checks. The first Alert asks for permission. A click opens the right PR from a banner and from Notification Center. Permission survives installing the next build.
- The README says that installing without the Start menu shortcut turns Alerts off on Windows.
- When Developer ID signing arrives, macOS moves to `UNUserNotificationCenter`, which allows replacing an Alert and a click after a restart.
