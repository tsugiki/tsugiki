# Making a release

The steps to follow as maintainer.
[ADR 0012](adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md) says why releases work this way,
and `.github/workflows/release.yml` holds the details.

## 1. Prepare the version

The version goes to main in a PR from a `release/X.Y.Z` branch, such as `release/0.2.4`. On a clean main
that matches GitHub's, `just prepare-release X.Y.Z` does the mechanical part and commits nothing:

1. It makes the branch `release/X.Y.Z`.
2. It sets `version` in `[workspace.package]` in `Cargo.toml`, and runs `cargo check` so `Cargo.lock`
   follows. The release builds use `--locked`, and they fail if it doesn't.
3. It renames `## WIP` in `CHANGELOG.md` to `## Version X.Y.Z (YYYY-MM-DD)`, with today's date, and starts a
   new empty `## WIP` above it.

Then edit the notes under the new heading by hand: ADR 0012 says what the section needs, and
`just release-check` prints the notes the release will carry. Commit, and run `just ship --release --check`,
which pushes the branch and opens the PR after `just check` passes. It stops before the push unless the
branch names `Cargo.toml`'s version and `just release-check` passes. Each push rehearses the release on CI
(see [Rehearsing](#rehearsing)), so an agent asks the user first. Plain `just ship` refuses a `release/`
branch, and `just ship --release` pushes a fix for a gate you reran yourself.

CI's `release-check` job fails the PR if `CHANGELOG.md` has no section for that version, and the `deny` job
fails it if `Cargo.lock` didn't follow.

If `just prepare-release` stops halfway, finish by hand: `git switch -c release/X.Y.Z`, edit the two files as
above, and run `cargo check`. If `just ship --release` can't push or open the PR,
`git push -u origin release/X.Y.Z` and `gh pr create --base main` do it.

## 2. Tag it

Once the PR has merged, run `just release` on main; an agent asks the user first, since the tag starts the
workflow. It fetches, then refuses unless the tree is clean and main matches `origin/main`. It takes the
version from `Cargo.toml`, so the tag can't disagree with the downloads, and refuses when `vX.Y.Z` exists in
this clone or on GitHub, or when `just release-check` fails. Then it tags HEAD, pushes the tag and prints
the link to the release run.

By hand, it does this:

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

The workflow can't do this, so you do it on the draft's zip: measure the performance budgets in the real
window on Windows
([ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).

Windows comes first. Until you take macOS on, the `.dmg` ships with CI's checks only. From then on, also run
the [macOS checklist](macos-checklist.md) on the `.dmg`, which measures the budgets there too
([ADR 0009](adr/0009-testing-and-full-coverage.md)).

## 4. Publish

Publish the draft on GitHub's release page. This can't be taken back: immutable releases stop anyone changing
a published release's files or tag, so to fix a bad release, make a new version.

## Rehearsing

Each push to a PR from a `release/` branch rehearses the release. A PR from another branch starts a Release
run that skips every job, and CI fails it if it sets a version or changes the workflow. To try a change to
the workflow, push it on a `release/` branch and open a PR. A rehearsal needs no tag. It checks that
`CHANGELOG.md` has a section for `Cargo.toml`'s version, runs the smoke test and builds the zip and the dmg,
then stops, so it leaves no attestation and no draft. A newer push cancels the PR's older rehearsal. You can
download both files from the run, but they have no attestation, so don't hand them to anyone.

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
