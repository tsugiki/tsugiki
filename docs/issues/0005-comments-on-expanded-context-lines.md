# Check whether GitHub takes line comments on expanded context lines

Status: open

The diff view can show hidden lines around each change, as on GitHub (scope.md, In-app review). The prototype assumes GitHub accepts line comments only on lines inside the diff's hunks. So clicking the line number of an expanded line opens no comment box, and says the line is only there for context.

This assumption hasn't been checked. If GitHub does accept such comments, gh-rev should allow them too.

## To find out

- Read the REST "Create a review comment for a pull request" and GraphQL `addPullRequestReviewThread` docs for what `line` / `side` may point at outside a hunk.
- Or check on github.com, without posting: expand context in a PR's Files tab and see whether the `+` comment button appears on the expanded lines.

## Then

If comments are allowed there, let the composer open on expanded lines and send them with the line's number at the PR's head (RIGHT side). If not, keep the current behaviour and give the reason in scope.md ("Line comments stay on lines of the diff", because GitHub requires it).
