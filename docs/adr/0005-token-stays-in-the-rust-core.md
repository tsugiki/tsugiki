# The GitHub token stays in the Rust core

Only the Rust side of Tsugiki holds the token and talks to GitHub. The webview asks it for data through Tauri
commands and never sees the token, never makes a request to GitHub itself, and has a strict content security
policy with no remote scripts. The webview shows HTML written by strangers: PR descriptions, comments and
Issue bodies. GitHub sanitizes it, but if anything slips through, it runs in a page that holds no token and
can reach no network but the hosts GitHub serves that HTML's images from. Keeping the token there would
otherwise rest on every sanitizer and every view being right.

This matters more because the default token (gh's, [ADR 0004](0004-auth-via-gh-cli-token.md)) can push to
`python/cpython`. A leaked token is a supply-chain risk for CPython, not only a privacy problem.

## Consequences

- Every GitHub call, REST or GraphQL, is a Rust command. That is more plumbing than calling the API from
  TypeScript, and it is the price.
- Rust sends HTTP requests to `api.github.com` only, all through `tsugiki-token`
  ([ADR 0019](0019-the-token-and-the-only-code-that-sends-it-have-a-crate-of-their-own.md), and
  [the crate's README](../../crates/token/README.md) for readers who don't read Rust), and the mirror's
  git fetches from `github.com`. No telemetry, no other servers; Tsugiki checks GitHub Releases for a new
  version and links to it ([ADR 0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).
- A token from `gh` is read at each start and never written anywhere. A personal access token is kept only in
  the OS keychain, never in the config files or the database, so it never travels with them ([ADR
  0014](0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)). It is one entry,
  service `io.github.tsugiki` and account `github.com`, written with the `keyring` crate; debug builds use
  `io.github.tsugiki.dev`. `machine.toml` records only where the token comes from. Tsugiki reads the entry
  once at start-up and keeps the token in memory; if the keychain refuses, it waits for the user's Try again
  rather than asking repeatedly. Logs and error reports scrub tokens. Reading the token, the keychain entry
  and the scrubbing live in `tsugiki-token`
  ([ADR 0019](0019-the-token-and-the-only-code-that-sends-it-have-a-crate-of-their-own.md),
  [README](../../crates/token/README.md)).
- git never gets the token for a public Repository: the mirror fetches anonymously.
- Tauri capabilities expose only Tsugiki's own commands to the webview: no shell, file system or HTTP plugins.
- The webview loads images, and nothing else, from the hosts GitHub serves the images of descriptions and
  comments from (#761). GitHub's rendered HTML sends each third-party image through camo, its image proxy,
  so a badge's owner never hears from the user's machine. Only GitHub does, which the core talks to anyway.
  The hosts are `camo.githubusercontent.com`; `github.githubassets.com` for custom emoji;
  `user-images.githubusercontent.com` and `private-user-images.githubusercontent.com` for uploaded images;
  and the GitHub hosts the renderer sends no further: `github.com` for a workflow's badge,
  `raw.githubusercontent.com` for a file of a Repository, `media.githubusercontent.com` for a Git LFS file
  and `avatars.githubusercontent.com`. Scripts, frames, fetches, forms and uploaded videos still reach
  nothing.
- The rendered HTML gives a newer uploaded image an address on `private-user-images` signed for five
  minutes, and signed afresh on each fetch. The open Issue or PR is fetched again about once a minute, so
  its images keep showing, and the view takes the fresh signatures without drawing the HTML again. An Issue
  or PR fetched more than five minutes before shows a broken image in place of each upload until a fetch
  brings fresh addresses: offline, until Tsugiki is back online. The upload's own address, under
  `github.com/user-attachments/`, redirects to an Amazon S3 host without GitHub's cookie, which the policy
  refuses.
- The webview can ask the core to open a link in the system browser, since a link in a description has to go
  somewhere. The core treats that link as untrusted and opens only an `http` or `https` address, never a file
  or an installed app's scheme.
- Releases are built in GitHub Actions with artifact attestations, so a user can check one with `gh
  attestation verify`. The source is public.
- The Account page says all of this in plain words, next to what the current token lets Tsugiki do.
