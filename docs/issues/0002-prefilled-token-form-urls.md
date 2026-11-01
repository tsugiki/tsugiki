# Check GitHub's pre-filled token forms while signed in

Status: open

The Account page's guides open GitHub's token forms already filled in:

- Read-only: `https://github.com/settings/personal-access-tokens/new?name=gh-rev+(read-only)&description=…&expires_in=90`
- Reviews: `…/personal-access-tokens/new?name=gh-rev+(reviews)&target_name=<owner>&expires_in=90&pull_requests=write&issues=write&contents=read`
- Classic: `https://github.com/settings/tokens/new?scopes=public_repo&description=gh-rev`

The parameters come from GitHub's documentation ("Managing your personal access tokens": pre-filling fine-grained token details with URL parameters). Nobody has opened these links while signed in.

## To find out

Open each link signed in to GitHub, and check that:

- the name, description, expiry and permissions are filled in as the guide's steps say;
- "Public repositories" is the default repository access, as the read-only guide assumes;
- `target_name=python` works for a member of the org. The docs say it fails for non-members.

Don't generate a token while checking.

## Then

Fix the URLs or the guide's steps where they differ, in the prototype (`prototype/app-shell/src/lib/core/tokens.ts`) and later in the app.
