# tsugiki-token

This crate is the part of Tsugiki that handles your GitHub token. It is small so that you can check what
Tsugiki does with the token without reading the rest of the app. The decision behind it is
[ADR 0019](../../docs/adr/0019-the-token-and-the-only-code-that-sends-it-have-a-crate-of-their-own.md).

Rust only lets code inside a crate see what the crate keeps private. The token's text is private to this
crate, so the rest of Tsugiki can pass the token around but can't read it, print it or save it. The compiler
enforces this, and the build fails if other code tries.

## Where the token comes from

Tsugiki runs `gh auth token --hostname github.com`, the [GitHub CLI](https://cli.github.com/)'s own command,
each time it starts. If `gh` isn't installed, fails, or prints nothing, Tsugiki has no token and asks you to
run `gh auth login`. The code is `Token::from_gh` in [`src/lib.rs`](src/lib.rs). Only one function in the
rest of Tsugiki may call it, and the lint settings in [`clippy.toml`](../../clippy.toml) fail the build on any
other caller. That function hands the log a copy of the token to hide.

A personal access token kept in the system keychain will come here too, when Tsugiki supports one.

## Where the token goes

The token goes to one place: the `Authorization` header of a request to `https://api.github.com`. The code is
`Https` in [`src/live.rs`](src/live.rs), the only HTTP client in Tsugiki. It:

- uses HTTPS only, sends GraphQL to one fixed address, and checks a REST address's host before it sends;
- follows no redirects, so GitHub's answer can't send the request, and the token, somewhere else;
- takes a REST path only as a `RestPath`, which refuses `@`, `?`, `#` and `\`, the characters that could make
  a path name another host. A query is encoded on its own, so it can't either.

## What Tsugiki asks without a token

Tsugiki sends GitHub one request with no token: `GET /repos/tsugiki/tsugiki/releases/latest`, which asks for
its latest release, at start-up and every 6 hours, so the status bar can say when a new version is out. The
code is `Https::get_public` in [`src/live.rs`](src/live.rs). It follows the rules above and sends no
`Authorization` header. It takes no token, so it has no way to send one, and a test checks the header isn't
there. `check = false` under `[releases]` in `settings.toml` turns the check off, and Tsugiki then never
sends this request.

## Where the token never goes

- **The disk.** Tsugiki writes the token to no file: not the settings, not the database, not the log.
- **The window.** The window is a web page that shows HTML that other people wrote, such as PR descriptions.
  The token can't be turned into the messages the window receives, so a page that went wrong has no token to
  leak.
- **The log.** Each line of Tsugiki's log passes through `Token::hide_in`, which replaces the token with
  `[token]`, and through `hide_token_shapes`, which also replaces anything that looks like a GitHub token
  (`ghp_…`, `gho_…`, `github_pat_…` and the rest). The tool that records GitHub's answers for the tests does
  the same.

## What this crate doesn't cover

- **The window's isolation.** That the window can't reach the network or run programs comes from Tauri's
  settings and the page's content security policy
  ([ADR 0005](../../docs/adr/0005-token-stays-in-the-rust-core.md)), not from this crate.
- **git.** Tsugiki fetches code with git, which never gets the token: it fetches without signing in. That
  code is in the core.
- **GitHub's answers.** What the rest of Tsugiki does with the data GitHub sends back is the core's business.
  This crate hands the answer over as text.

## How to check it

- The crate's dependencies are listed in [`Cargo.toml`](Cargo.toml): `ureq` (the HTTP client), `rustls`
  (to tell an expired certificate from other failures) and `url`. `just lint` fails if one is added, so a new
  one needs a reviewed change to that list.
- `cargo tree -i ureq` shows which crates use the HTTP client. `tsugiki-token` is the one on its first level,
  and the others reach it through `tsugiki-token`.
- The token's text is the `String` inside `pub struct Token(String)` in [`src/lib.rs`](src/lib.rs). Search
  this crate for `.0` on a `Token`: the header and the hiding are the only places that read it.
