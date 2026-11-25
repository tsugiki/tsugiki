# Status bar

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A **status bar** runs along the bottom of every screen, the Home page included, for what is app-wide and has
  no other place. It stays when the Sidebar is hidden.
- Left, where the eye spends its time, the status, which a screen reader announces as it changes. It has
  two parts, the account and a problem, so a problem can't hide who is signed in or why there is no token:
  - The account, which reads of the token and GitHub's answers change, and a problem never does:
    - Who is signed in, once GitHub has said. Before it answers, the login of the last run with
      "Connecting…", or "Connecting…" alone. A Refresh while offline says "Connecting…" too, until GitHub
      answers or the ask fails, and so does a Refresh without a token, until `gh` answers.
    - **Offline**, after the login of the last run, if any, while GitHub can't be reached. It tints the
      whole bar.
    - In place of a login, the token's problems in the core's words: a read that found none, or a token
      GitHub refused, each opening the Settings page that fixes it.
    - The login of the last run alone, or nothing, once GitHub answered without saying who is signed in,
      such as with a 5xx or a query it refused. Nothing is still connecting, and the problem says what went
      wrong. A certificate this computer's clock doesn't accept stops the request before GitHub answers,
      so it leaves the account as it was, offline again after a Refresh that asked while offline.
    - Nothing, once Tsugiki asks GitHub nothing more this run: its database didn't open, or a thread
      stopped. The problem says why, and stands alone.
  - **Read-only**, after the account, while the open Repository is Read-only, for the token or by the
    user's choice ([token.md](token.md)). A hidden bar leaves it out beside a token's problem, and the bar
    leaves it out once Tsugiki asks GitHub nothing more.
  - After Read-only, a problem that stops Tsugiki reading, such as a store write that failed, in the
    core's words. Each finished status check or read of the token takes it away or puts a new one in its
    place.
  - A read that found no token has Try again after the status, the problem included, which reads the token
    at once ([token.md](token.md)), on the Home page and on a first run with no Repository added too.
    Nothing else has it, not even a refused token.
  - After the status, each problem with a file Tsugiki reads, such as a line of `settings.toml` it can't
    parse, with the file and the line; the wait for GitHub's points ("Sync paused until 14:00: GitHub's rate
    limit is low", or with no sync waiting, "GitHub's rate limit is low. Tsugiki can ask it again at
    14:00"); and why no mirror can be made, such as a missing git or a disk without room for a clone, with
    Get git and Check again beside it.
  - The first Repository whose mirror waits to hear where it gets its objects ("python/cpython's mirror
    waits for your answer"), which a click asks.
- Right:
  - A newer version ("Tsugiki 0.4.0 is out"), which a click opens on its release page.
  - The request to GitHub in flight, once it has gone 3 s without an answer: what Tsugiki asked and the
    seconds since, counting up ("Waiting for GitHub: refresh of python/cpython, 4 s"). Tsugiki sends one
    request at a time, and a request can take up to 15 s to fail, so the user's own asks wait behind it. A
    sync sends about one request a second, so the part leaves out those that answer sooner. It names the
    status check, a refresh or a first sync of a Repository, a read of an Issue or a PR on screen, quick
    open's fetch of a number, Add repository's check, the size the Mirror worker asks before a clone, or
    whether a Repository is private, which Tsugiki asks of a token with `public_repo` alone. The
    part keeps one width whatever it names, narrower in a narrow window so the status keeps its room. A long
    subject ends in an ellipsis before the seconds, and the tooltip holds the whole text. It goes once GitHub
    answers or the request fails, and the status shows a failure, such as Offline. While Tsugiki asks `gh`
    for the token, which no request can go without, it says "Waiting for gh to give the token, 4 s" in
    the same place ([token.md](token.md)).
  - Sync status, which a click refreshes: "Synced 2 min ago", with the date and time of the last sync in
    its tooltip, or "Not synced yet" for the open Repository, and a first sync's part and progress in its
    place ([sync.md](sync.md)). The background work follows
    ("· python/peps syncing", "· Cloning python/cpython 42%", "· Cloning python/cpython starts once GitHub
    answers"). It shows once a Repository is open.
  - Unsent **Pending reviews** in any Repository, which a click opens on the PR.
  - The hour's GitHub points ("1,240 points left · resets 14:32"). The part shows from GitHub's first answer
    of the run, since until then Tsugiki doesn't know them, and goes at the reset, by GitHub's clock, until
    the next answer. Its tooltip, which a screen reader reads with the part, says what Tsugiki spent: "This
    hour: 3,760 of 5,000 points used. Tsugiki spent 3,610 of them; other tools using your token spent the
    rest." Tsugiki counts in memory only, so after a restart in the middle of the hour it says "Tsugiki
    spent 410 since 14:05".
  - The first key of a chord while Tsugiki waits for the second.
- Each part can be switched off from the bar's right-click menu or Settings › General, and the whole bar can
  be hidden ("View: Toggle Status Bar"). Being offline, the token's problems, a problem, a file Tsugiki
  can't read, the wait for GitHub's points, why no mirror can be made and a mirror's question still show,
  since nothing else would tell the user: the bar comes back with only those, with Try again, Get git and
  Check again beside theirs, and with sync status beside being offline or a problem.
