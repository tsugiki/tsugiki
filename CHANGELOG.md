# Changelog

Release notes for Tsugiki. Keep "WIP" current as changes land, and rename it to
`## Version X.Y.Z (YYYY-MM-DD)` in the PR that sets the version. The release workflow uses that section as the
release's body, and fails without it
([ADR 0012](docs/adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## WIP

### Added

- A window that follows whether GitHub can be reached and who is signed in.
- `tsg` runs one Tsugiki and passes `tsugiki://` links and other arguments to it.
- Tsugiki's keys reach Tsugiki: Print, Reload and the other browser shortcuts stay off.
- A first sync also loads the last 30 days in the background, shows its progress in the status bar, and
  picks up where it stopped.
- A portable `.zip` for Windows and a `.dmg` for Apple silicon Macs. An installer comes later, with Alerts.
