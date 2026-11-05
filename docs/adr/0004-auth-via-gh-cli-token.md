# Authenticate by reusing the gh CLI's token

Tsugiki gets its GitHub token from `gh auth token` at startup, and falls back to a personal access token
entered in settings on machines without `gh`. It does not register its own OAuth app. An OAuth app (or a
fine-grained PAT) acting on `python/cpython` may need the python organization's approval under its third-party
app policy, whereas `gh`'s OAuth app is already trusted wherever a core developer uses it. This also avoids a
second login. The cost is that Tsugiki depends on `gh` being installed and has exactly the scopes `gh` was
granted.

## Amendment: narrower tokens are supported, not only tolerated

gh's token is broad. `gh auth login` usually grants `repo`, `read:org`, `gist` and `workflow`, and `repo` can
push to every repository the user can push to; for a core developer that includes `python/cpython`. Someone
trying Tsugiki, or a careful user, should not have to hand it that. So gh's token stays the default, and a
personal access token becomes a supported choice with its trade-offs stated in the app:

- **Fine-grained, public repositories read-only.** No permissions, no organization approval. Everything
  Tsugiki shows works; nothing can be written. This is the token to try Tsugiki with.
- **Fine-grained, one owner's repositories, Pull requests and Issues: write.** Reviews and comments, but no
  push and no merge. Its resource owner is the Repository's owner, so for `python/cpython` the python
  organization may have to approve it; until then GitHub lets it read only.
- **Classic, `public_repo`.** Writes to any public repository and needs no approval, but can push wherever the
  user can.

GitHub's token forms can be pre-filled through URL parameters
(`/settings/personal-access-tokens/new?name=…&target_name=…&pull_requests=write`,
`/settings/tokens/new?scopes=public_repo`), so the Account page links straight to a filled-in form. The
repository selection of a fine-grained token can't be pre-filled.

A fine-grained token can't be asked what it allows. Tsugiki learns that it can't write to a Repository when
GitHub refuses a write: the text being sent stays in the composer with "Copy and open on GitHub", and the
Repository is **Read-only** from then on. gh's and classic tokens report their scopes, so Tsugiki knows up
front.

A fine-grained token that writes covers one owner's repositories. Repositories of several owners with write
access each would need a token per owner; that is Later.
