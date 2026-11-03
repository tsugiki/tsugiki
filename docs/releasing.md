# Making a release

The steps to follow as maintainer.
[ADR 0012](adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md) says why releases work this way,
and `.github/workflows/release.yml` holds the details.

## 1. Prepare the version

In a PR to main:

1. Set `version` in `[workspace.package]` in `Cargo.toml`, and run `cargo check` so `Cargo.lock` follows. The
   release builds use `--locked`, and they fail if it doesn't.
2. Rename `## WIP` in `CHANGELOG.md` to `## Version X.Y.Z (YYYY-MM-DD)`, with today's date, and start a new
   `## WIP` above it. ADR 0012 says what the section needs.

`just release-check` prints the notes the release will carry. CI's `release-check` job fails the PR if
`CHANGELOG.md` has no section for that version, and the `deny` job fails it if `Cargo.lock` didn't follow.

## 2. Tag it

Once the PR has merged:

```
git switch main
git pull --ff-only
git tag vX.Y.Z
git push origin vX.Y.Z
```

The tag starts the workflow, which takes 16 minutes or longer if runners are busy. It builds the Windows zip
and the macOS dmg, attests them and uploads them to a draft release, then downloads the draft's files and
checks each attestation. Nothing is public until you publish.

## 3. Check the draft

The workflow can't do these, so you do them on the draft's files:

- Run the macOS checklist on the `.dmg` ([ADR 0009](adr/0009-testing-and-full-coverage.md)). It isn't written
  yet.
- Measure the performance budgets in the real window on Windows and macOS
  ([ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).

## 4. Publish

Publish the draft on GitHub's release page. This can't be taken back: immutable releases stop anyone changing
a published release's files or tag, so to fix a bad release, make a new version.

## Rehearsing

To try the workflow without a release, for example after you change it, run it on a branch:

```
gh workflow run release.yml --repo tsugiki/tsugiki --ref <branch>
```

Or use "Run workflow" on the Release workflow in the Actions tab. A rehearsal needs no tag. It checks that
`CHANGELOG.md` has a section for `Cargo.toml`'s version, runs the smoke test and builds the zip and the dmg,
then stops, so it leaves no attestation and no draft. You can download both from the run, but they have no
attestation, so don't hand them to anyone.

## If the workflow fails

| What failed | What to do |
|---|---|
| The tag is on main | You tagged another branch. Delete the tag, and tag main. |
| The tag matches `Cargo.toml`, and `CHANGELOG.md` has its section | Fix them in a PR, delete the tag, and tag the merged commit. |
| The smoke test, or a build | Fix it in a PR, delete the tag, and tag the merged commit. If the job looks flaky, re-run it. |
| No release has this tag yet | An earlier run left a draft. Look at it, delete it, and re-run. |
| Verifying the draft's files | Don't publish. Delete the draft, and find out why before you tag again. |

To delete a tag, and a draft when one exists:

```
gh release delete vX.Y.Z --repo tsugiki/tsugiki --yes
git push origin --delete vX.Y.Z
git tag -d vX.Y.Z
```

Each run that gets past the attest job leaves its attestations in a public log for good. They truthfully
describe files that nobody published, so they do no harm.
