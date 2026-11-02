## Agent skills

### Issue tracker

GitHub Issues on tsugiki/tsugiki, through the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default roles, each label named after its role: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` and `docs/adr/` at the repo root, plus `docs/scope.md` and `docs/build-order.md`. See `docs/agents/domain.md`.

## Writing prose

Check docs, commit messages, issues and PRs with `/stop-slop` before you publish them.

Wrap the docs you write at 110 columns: Markdown files, comments and docstrings. Leave tables unwrapped,
and rewrap only the paragraphs you change.
