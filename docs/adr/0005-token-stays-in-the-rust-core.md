# The GitHub token stays in the Rust core

Only the Rust side of Tsugiki holds the token and talks to GitHub. The webview asks it for data through Tauri commands and never sees the token, never makes a request to GitHub itself, and has a strict content security policy with no remote scripts. The webview shows HTML written by strangers: PR descriptions, comments and Issue bodies. GitHub sanitizes it, but if anything slips through, it runs in a page that holds no token and can reach no network. Keeping the token there would otherwise rest on every sanitizer and every view being right.

This matters more because the default token (gh's, [ADR 0004](0004-auth-via-gh-cli-token.md)) can push to `python/cpython`. A leaked token is a supply-chain risk for CPython, not only a privacy problem.

## Consequences

- Every GitHub call, REST or GraphQL, is a Rust command. That is more plumbing than calling the API from TypeScript, and it is the price.
- Rust connects to `api.github.com` and `github.com` only. No telemetry, no other servers; updates come from GitHub Releases.
- A token from `gh` is read at each start and never written anywhere. A personal access token is kept only in the OS keychain, never in the config files (the portable one is synced by the user). Logs and error reports scrub tokens.
- git never gets the token for a public Repository: the mirror fetches anonymously.
- Tauri capabilities expose only Tsugiki's own commands to the webview: no shell, file system or HTTP plugins.
- Releases are built in GitHub Actions with artifact attestations, so a user can check one with `gh attestation verify`. The source is public.
- The Account page says all of this in plain words, next to what the current token lets Tsugiki do.
