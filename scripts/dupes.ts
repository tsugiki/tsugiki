// `just dupes`: fails when two files hold the same block of code, as AGENTS.md's "One copy" rule forbids.
// jscpd finds the copies, and copies.ts holds the scans and the repeats that stay.
import { mkdirSync } from "node:fs";
import { join } from "node:path";

import { ALLOWED, describeCopy, find, judge, SCANS } from "./copies.ts";
import { stop } from "./run.ts";

const found = SCANS.flatMap((scan) => {
  const output = join(".temp", "jscpd", scan.name.replaceAll(" ", "-").toLowerCase());
  mkdirSync(output, { recursive: true });
  return find(scan, process.cwd(), output);
});
const { copies, unused } = judge(found, ALLOWED);
for (const copy of copies) {
  console.error(describeCopy(copy));
}
for (const { files } of unused) {
  console.error(`copies.ts allows a repeat between ${files.join(" and ")} that no longer exists. Remove it.`);
}
if (copies.length > 0) {
  console.error(
    `${String(copies.length)} blocks of code have a copy in another file. Move each to one place both use, or ` +
      "add the pair to ALLOWED in scripts/copies.ts with why it stays.",
  );
}
if (copies.length > 0 || unused.length > 0) {
  stop("The dupes gate failed.");
}
console.log(
  `No block of code has a copy in another file, besides those between the ${String(ALLOWED.length)} pairs of ` +
    "files that copies.ts allows.",
);
