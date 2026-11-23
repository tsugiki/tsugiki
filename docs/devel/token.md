# Token

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The GitHub token comes from `GH_TOKEN` when it is set and not empty, and from `gh auth token` otherwise.
  `gh auth token` gives `GH_TOKEN` first too, so the token is the same, but Tsugiki then runs no `gh`. A
  personal access token is a supported choice: fine-grained read-only to try Tsugiki, fine-grained with
  review rights on one owner's repositories, or classic `public_repo`
  ([ADR 0004](../adr/0004-auth-via-gh-cli-token.md)).
- After a read that found no token, Tsugiki reads it again about once a minute, with the check of who is
  signed in, and at once on a click of Try again or of an open Repository's sync status. Try again follows
  the status in the status bar on every screen, with or without a Repository open, and another problem
  shown after the missing token leaves it there ([status-bar.md](status-bar.md)). A click while a read is
  out adds no read. Once a read gives a token, Tsugiki connects as it does at start, with no restart, and
  reads the items on screen at once. A click says "Connecting…" until the read ends; the minute's read
  leaves the missing token up until it has one. Tsugiki waits on `gh` meanwhile and hears no click until the
  read ends, so a click while it is out reads nothing again: a read that finds no token covers the click,
  and after one that finds a token, the click asks GitHub as a click on the sync status does. The log gives
  a failed read's cause once, and again when a read fails another way.
- Without a token, the status bar says what happened, what Tsugiki does next and what the user can do. With
  no `gh`, or a `gh` that isn't signed in: install `gh` or run `gh auth login`, and Tsugiki asks again. A
  `gh` that doesn't answer within 30 s is installed and may be signed in, so Tsugiki stops it, and the
  status bar says `gh auth token` didn't answer, that a keychain or credential manager waiting on a prompt
  is a common cause, and that Tsugiki asks again. Both name `GH_TOKEN` as the way past `gh`, with a restart:
  Tsugiki reads its own environment, which a later change in the user's shell doesn't reach.
- While a read has gone 3 s without an answer, the status bar says Tsugiki is waiting for `gh` to give the
  token, and for how long ([status-bar.md](status-bar.md)).
- Only the Rust core holds the token and talks to GitHub; the webview never sees it ([ADR
  0005](../adr/0005-token-stays-in-the-rust-core.md)).
- Tsugiki sends GitHub GraphQL reads and REST GETs, and a write only where `tsugiki-token` lists it ([ADR
  0025](../adr/0025-tsugiki-token-lets-through-reads-and-the-writes-it-lists.md)). A GraphQL body that holds
  `mutation` never leaves: the request fails on its own, as one GitHub refused would, and the log names it
  and says why.
- The **Account** page shows who is signed in, where the token comes from, and what it lets Tsugiki do in
  plain words ("read public repositories", "write reviews and comments", "push to any repository you can
  push to"), with gh's or a classic token's scopes. It says what Tsugiki does with the token (ADR 0005):
  only the Rust core holds it, it goes only to `api.github.com`, and it is never written to the settings
  files, the database or the log. It links the [README](../../crates/token/README.md) of `tsugiki-token`
  on GitHub, at the release's tag, for readers who want to check. A guide makes a narrower token:
  read-only, reviews on one owner's repositories, or classic `public_repo`, each with steps, what it can and
  can't do, and a link to GitHub's form already filled in. A pasted token is checked before it is stored,
  and the page shows its kind, source and expiry, never the token itself; within 7 days of expiry the
  status bar warns and opens the guide. Replace and Remove act on the keychain entry, and Remove falls back
  to gh when gh is signed in. If the keychain refuses the token or is locked, Tsugiki shows its cached data
  without syncing, and the status bar says so with Try again; the Account page offers gh or a new token. A
  machine with no token, such as a new one or a portable folder brought there, says "No token on this
  machine".
- A token that can't write to a Repository makes it **Read-only**, which being offline doesn't
  ([sync.md](sync.md)). GitHub names a classic token's scopes in a header of each answer, reads included: a
  token without `repo` or `public_repo` can't write anywhere, and one with `public_repo` alone can't write
  to a private Repository, which Tsugiki asks GitHub once a run. Only an OAuth or a classic token's scopes
  count: a fine-grained token sends none, and the scopes of another kind don't limit it. The database keeps
  each Repository's mark with the token's kind and expiry, never the token, so the mark outlasts a restart
  and goes as soon as Tsugiki sees another token. The Issue and PR views of a Read-only Repository say
  "Read-only: your token can't write to python/cpython" atop, and the status bar says "Read-only" on the
  left, after who is signed in.
- A Repository is **Read-only** also when the user chose it, in Add repository or on its page in Settings,
  which `settings.toml` keeps ([settings.md](settings.md)).
  Tsugiki sends no write to a Read-only Repository, whichever the reason. As a write is about to go, the
  core works out from the settings and the database whether the Repository is Read-only, so a write that a
  control let through, or one sent after the mark changed, still goes nowhere. Such a write fails as one
  GitHub refused does, saying why ("Tsugiki sent nothing, as your token can't write to python/cpython."),
  and the composer keeps its text. A write names its Issue or PR by the ID GitHub gave on the item's last
  read, which the database keeps for that number in that Repository; before Tsugiki has read the item, the
  write fails unsent and asks the user to open it again.
- Later tickets build on the mark. Settings › Account lists the Read-only Repositories, and the status bar's
  Read-only opens it (#434). The composer stays usable: it offers "Copy and open on GitHub" instead of
  sending, and Submit review is disabled with the reason. With a fine-grained token Tsugiki finds out on the
  first refused write, keeping the text that was being sent (#733).
