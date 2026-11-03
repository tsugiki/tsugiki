// Checks a version against CHANGELOG.md and prints its release notes. The release workflow runs it first, so
// a bad tag fails before anything is built (ADR 0012).
//
// `node scripts/release-check.ts v1.2.3` checks a tag, and that it matches Cargo.toml.
// `npm run release-check` checks the version Cargo.toml says. CI runs this on PRs, so a PR that sets a
// version fails without its changelog section, before anyone tags it.
import { readFileSync } from "node:fs";

import { notesFor, notesOfWorkspace, versionOfTag } from "./release-notes.ts";

try {
  const [tag] = process.argv.slice(2);
  const cargoToml = readFileSync("Cargo.toml", "utf8");
  const changelog = readFileSync("CHANGELOG.md", "utf8");
  console.log(
    tag === undefined
      ? notesOfWorkspace(cargoToml, changelog)
      : notesFor(changelog, versionOfTag(tag, cargoToml)),
  );
} catch (cause) {
  console.error(cause instanceof Error ? cause.message : String(cause));
  process.exit(1);
}
