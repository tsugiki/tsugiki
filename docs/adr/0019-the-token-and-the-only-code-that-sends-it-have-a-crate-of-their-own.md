# The token and the only code that sends it have a crate of their own

A user who wants to know what Tsugiki does with their GitHub token shouldn't have to read the core, with its
store, sync and highlighting. So `crates/token` (package `tsugiki-token`) holds where a token comes from, the
one place it goes, and how a secret is hidden. Rust's privacy works at the crate's edge, so the compiler keeps
the token on that path. The crate will have a README that says this for someone who doesn't read Rust, and the
top-level README and [ADR 0005](0005-token-stays-in-the-rust-core.md) will link to it.

The crate holds:

- `Token`, with no way to read its string from outside the crate. It has no serde, so no command or event can
  carry it, and its `Debug` prints `Token(***)`. It is `Clone`, so the logger can hold a copy.
- `Token::from_gh`, which runs `gh auth token --hostname github.com` and returns `NoToken` (gh is missing,
  failed, or printed nothing) when it can't. `run_gh` and the places it looks for `gh` come with it. The
  personal access token ([ADR 0004](0004-auth-via-gh-cli-token.md)) joins it here, so `keyring` is a
  dependency of this crate only.
- `Token::hide_in` and `hide_token_shapes`, which the logger and the GraphQL recorder use to take a token out
  of text, by its value and by its `ghp_`, `gho_` or `github_pat_` shape.
- `Https`, the only HTTP client in Tsugiki. It reaches `api.github.com` over HTTPS and follows no redirects.
  `post_graphql` takes a body as text and `get_rest` takes a `RestPath`, which the crate parses so that no
  path can name another host. Each puts the token in the `Authorization` header and returns the status, the
  body and the headers as text. A request that needs no token, such as the check for a new release
  ([ADR 0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)), goes through it too, with the
  same rules.

The core keeps the GraphQL client, the queries and the parsing of replies. It implements its `Transport`
trait for `Https`, so its test doubles stay as they are, and it still hands `&Token` to them. Its `read_token`
is the only caller of `Token::from_gh`: it logs why gh gave no token, shows the sign-in message, and gives the
logger a copy of the token.

## Considered Options

- **Only the token in the crate**, with the HTTP client left in the core. The token's getter would stay
  public, so an auditor would still search the core for each place that reads the string.
- **The GraphQL client in the crate as well.** It would bring serde, GitHub's clock and the queries along,
  and the crate would grow back into the core's `github` module.
- **A signed-in client** that holds the token, so `Transport` and `GitHub` take none. The core can't read an
  opaque `Token`, so this adds no guarantee, and it would change about 20 test doubles and 25 builders.
- **A second crate for requests without a token.** `get_public` takes no `Token`, so it can't send one
  already. A second crate would repeat the client's rules and give an auditor two clients to read.
- **Publishing the crate on crates.io.** Its API will change until 1.0, and publishing would make it a second
  thing to release.
- **`expose` private to `token.rs` inside the core.** One file to read, but no crate whose dependencies an
  outside reader can list.

## Consequences

- `tsugiki-core` no longer depends on `ureq` or `rustls`. `cargo tree -i ureq` shows that only
  `tsugiki-token` links an HTTP client, and `cargo machete` notices if the core takes one up again.
- `clippy.toml` sends the network and `gh` to `crates/token/src/live.rs`, and `git` to the core's
  `github/live.rs`. A `disallowed-methods` entry keeps `Token::from_gh` inside `read_token`.
- `crates/token/src/lib.rs` keeps the 100% coverage gate
  ([ADR 0009](0009-testing-and-full-coverage.md)). `live.rs` is left out of it, as the core's was, and the
  release checks exercise it.
- The crate stays small by rule. Code belongs in it only if it decides where a token comes from, where bytes
  go on the network, or how a secret is hidden. GraphQL, JSON, reply parsing, GitHub's clock, rate limits,
  retries, domain types, UI text and logging belong in the core. `just lint` checks the crate's direct
  dependencies against a list (`ureq`, `rustls`, `url`, and `keyring` once the personal access token comes),
  so a new one needs a reviewed change to that list.
- The crate stays in the workspace until 1.0. Publishing it is worth asking again once its API settles.
- A request that needs another network rule, such as following a redirect to download a file, is the reason
  to split the client from the token.
