# Token

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- The GitHub token comes from `gh auth token` by default. A personal access token is a supported choice:
  fine-grained read-only to try Tsugiki, fine-grained with review rights on one owner's repositories, or
  classic `public_repo` ([ADR 0004](../adr/0004-auth-via-gh-cli-token.md)).
- Only the Rust core holds the token and talks to GitHub; the webview never sees it ([ADR
  0005](../adr/0005-token-stays-in-the-rust-core.md)).
- The **Account** page shows who is signed in, where the token comes from, and what it lets Tsugiki do in
  plain words ("read public repositories", "write reviews and comments", "push to any repository you can
  push to"), with gh's or a classic token's scopes. It says what Tsugiki does with the token (ADR 0005). A
  guide makes a narrower token: read-only, reviews on one owner's repositories, or classic `public_repo`,
  each with steps, what it can and can't do, and a link to GitHub's form already filled in. A pasted token
  is checked before it is stored, and the page shows its kind, source and expiry, never the token itself;
  within 7 days of expiry the status bar warns and opens the guide. Replace and Remove act on the keychain
  entry, and Remove falls back to gh when gh is signed in. If the keychain refuses the token or is locked,
  Tsugiki shows its cached data without syncing, and the status bar says so with Try again; the Account page
  offers gh or a new token. A machine with no token, such as a new one or a portable folder brought there,
  says "No token on this machine".
- A token that can't write to a Repository makes it Read-only too, as being offline does
  ([sync.md](sync.md)). The status bar says so on the left and
  opens the Account page. The composer stays usable: it offers "Copy and open on GitHub" instead of sending,
  and Submit review is disabled with the reason. With a fine-grained token Tsugiki finds out on the first
  refused write, keeping the text that was being sent.
