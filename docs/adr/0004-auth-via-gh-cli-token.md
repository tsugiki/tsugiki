# Authenticate by reusing the gh CLI's token

gh-rev gets its GitHub token from `gh auth token` at startup, and falls back to a personal access token entered in settings on machines without `gh`. It does not register its own OAuth app. An OAuth app (or a fine-grained PAT) acting on `python/cpython` may need the python organization's approval under its third-party app policy, whereas `gh`'s OAuth app is already trusted wherever a core developer uses it. This also avoids a second login. The cost is that gh-rev depends on `gh` being installed and has exactly the scopes `gh` was granted.
