# Check the python organization's policy on personal access tokens

Status: open

[ADR 0004](../adr/0004-auth-via-gh-cli-token.md) supports narrower tokens. Two of them depend on a policy of the `python` organization that we haven't seen:

- A fine-grained token whose resource owner is `python` (the "Reviews on one owner's repositories" guide) may need an organization owner's approval. Until approved, GitHub lets it read only.
- An organization can also block classic personal access tokens, which would stop the `public_repo` guide from writing to `python/cpython`.

The Account page and the docs say "may" for both. Tsugiki handles either case: a refused write keeps the text and makes the Repository **Read-only**.

## To find out

- Ask a `python` org owner, or check the org's settings page "Personal access tokens", what it requires for fine-grained tokens and whether it restricts classic ones.
- Or try it: create a fine-grained token for `python` with Pull requests and Issues write, then check whether it shows as pending.

## Then

Change "may" to what is true in the Account page's guides and in ADR 0004. If approval is required, say how long it usually takes, or who approves it.
