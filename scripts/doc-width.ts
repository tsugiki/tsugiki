// `just doc-width`: fails when a line of Markdown, a comment or a docstring runs past 110 columns. It reads
// what git tracks and what git would add, so a new file counts before its first commit.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

import { isDocFile, WIDEST, wideLines } from "./doc-lines.ts";

const listed = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"], {
  encoding: "utf8",
  maxBuffer: 2 ** 28,
});
const paths = listed.split("\0").filter((path) => isDocFile(path) && existsSync(path));
const wide = paths.flatMap((path) =>
  wideLines(path, readFileSync(path, "utf8")).map(({ at, width }) => `${path}:${at} is ${width} columns`),
);
for (const line of wide) {
  console.error(line);
}
if (wide.length > 0) {
  console.error(`${wide.length} lines run past ${WIDEST} columns. Wrap them.`);
  process.exit(1);
}
console.log(`${paths.length} files fit in ${WIDEST} columns.`);
