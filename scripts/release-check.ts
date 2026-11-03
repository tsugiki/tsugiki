// Checks a tag against Cargo.toml and CHANGELOG.md, and prints the release notes. `node scripts/release-check.ts
// v1.2.3`. The release workflow runs it first, so a bad tag fails before anything is built (ADR 0012).
import { readFileSync } from "node:fs";

import { notesFor, versionOfTag } from "./release-notes.ts";

try {
  const version = versionOfTag(process.argv[2] ?? "", readFileSync("Cargo.toml", "utf8"));
  console.log(notesFor(readFileSync("CHANGELOG.md", "utf8"), version));
} catch (cause) {
  console.error(cause instanceof Error ? cause.message : String(cause));
  process.exit(1);
}
