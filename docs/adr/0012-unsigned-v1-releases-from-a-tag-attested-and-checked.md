# Unsigned v1 releases from a tag, attested and checked before publishing

Releases are built from a tag in GitHub Actions and attested ([ADR 0005](0005-token-stays-in-the-rust-core.md)). In v1 the installers are not code-signed, and Tsugiki doesn't update itself: it tells the user a new version is out and links to the release. The attestation is what proves where a file came from, so the workflow checks it on the very files people download before anything is published.

## Releases

- **What ships.** A Windows x64 NSIS installer for the current user only, so it needs no admin rights. It installs into `%LOCALAPPDATA%\Programs\Tsugiki`, apart from the data ([ADR 0014](0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)), and adds its folder to the user's PATH for `tsg` ([ADR 0007](0007-core-library-and-thin-tauri-app.md)). A Windows portable `.zip` for portable mode. On macOS, one universal `.dmg` for Apple silicon and Intel.
- **Signing.** None on Windows, so installing shows SmartScreen's "Windows protected your PC" (More info, Run anyway). On macOS the app is only ad-hoc signed, so the first open needs System Settings › Privacy & Security › Open Anyway. The ad-hoc signature changes with every build, so a personal access token kept in the keychain asks for access again after an update. gh's token isn't stored there and isn't affected.
- **The workflow**, `release.yml`, runs in this order:
  1. Pushing a tag `vX.Y.Z` on main starts it.
  2. The pre-release suites of [ADR 0009](0009-testing-and-full-coverage.md) run: the CPython mirror suite, the `tauri-driver` smoke tests and the live-fixture check.
  3. The installers are built.
  4. Each installer is attested with `actions/attest-build-provenance` and uploaded to a draft release. Its body is the version's section of `CHANGELOG.md`, and the workflow fails if that section is missing.
  5. A last job downloads every asset of the draft and runs `gh attestation verify <file> --repo tsugiki/tsugiki --signer-workflow tsugiki/tsugiki/.github/workflows/release.yml` ([Verify release attestations in the release workflow before publishing](https://github.com/tsugiki/tsugiki/issues/6)).
  6. The maintainer runs the macOS checklist on the draft's `.dmg`, measures the performance budgets in the real window on Windows and macOS ([ADR 0017](0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)), and publishes the release by hand.
- The repository has GitHub's immutable releases turned on, so a published release's assets and tag can't change.
- **Update notices.** Tsugiki asks `api.github.com/repos/tsugiki/tsugiki/releases/latest` at start-up and every 6 hours. When that release is newer, the status bar says "Tsugiki 0.3.0 is out", and a click opens the release page in the browser. A switch in Settings turns the check off.
- **Versions** follow SemVer. 0.x releases are working builds while v1 is built, and 1.0.0 is the release that reaches it. After that, a minor version adds features, a patch fixes bugs, and a major one breaks something larger, such as dropping a platform. A database migration ([ADR 0010](0010-one-sqlite-database-and-per-repository-mirrors.md)) can come in any minor version.
- **Release notes** live in `CHANGELOG.md`. Its "Unreleased" section is kept current as changes land and is renamed to the version when it is tagged. A version's section has Added, Changed and Fixed in the glossary's words. It has a Database part when a migration runs, saying that older versions can't open the file and whether the cache is synced again. It ends with the first-install steps for SmartScreen and Open Anyway.

## Considered Options

- **Windows signing.** SignPath Foundation signs open-source projects for free, but the publisher reads "SignPath Foundation". Azure Artifact Signing is cheap but takes individual developers only in the USA and Canada. An OV certificate costs about $150–300 a year, its key must live on hardware or in a cloud service, and SmartScreen still warns until reputation builds. None is worth it while Tsugiki's users are the maintainer and a few colleagues.
- **Developer ID signing and notarization** with the maintainer's existing Apple Developer membership. It would remove the Gatekeeper block and the keychain prompt after updates, and costs nothing extra. It is put off, not rejected.
- **Tauri's updater**, checking for, downloading and installing a signed update in the app. It would need its own signing key, kept safe for as long as Tsugiki ships, since a lost key strands every installed copy. It would also need `release-assets.githubusercontent.com`, a host ADR 0005 doesn't allow. Downloading in the background and installing on quit was weighed as part of it.
- **An MSI installed for all users**, friendlier to managed machines, but every update needs an admin. **Apple silicon only**, a smaller download, but no Intel Macs.
- **Starting the release from the Actions tab**, with the workflow making the tag, or **publishing automatically** once verification passes. The first means a tag the maintainer didn't push; the second skips the macOS checklist.
- **CalVer**, which says how old a build is but not what changed. **Starting at 1.0.0**, with no working builds before v1. **Notes written in the draft**, or **generated from commits** with git-cliff, which would need a commit-message style Tsugiki doesn't use.

## Consequences

- No signing secrets and no updater key exist in v1. The release workflow needs only `contents: write`, `id-token: write` and `attestations: write`.
- The README and each release note explain the SmartScreen and Open Anyway steps, and how to check a download with `gh attestation verify`.
- Moving to notarization, Windows signing or in-app updates later changes only the workflow's build step and the update notice. Signing must still run before attestation, as the attestation ticket says.
