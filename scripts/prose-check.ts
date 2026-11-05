// `just prose-check` lists lines to reread against /stop-slop: the comments, docstrings and Markdown this
// branch adds compared with main. `just prose-check draft.md` reads the whole of a file, such as a PR body
// you drafted. It always exits 0, because a match is a reason to look.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

import { isDocFile } from "./doc-lines.ts";
import { addedLines, findings, RULES } from "./prose.ts";

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 2 ** 28 });
}

/** The lines to read in each file. `undefined` means all of them. */
function targets(named: string[]): Map<string, Set<number> | undefined> {
  if (named.length > 0) {
    return new Map(named.map((path) => [path, undefined]));
  }
  const added = addedLines(git("diff", "--merge-base", "origin/main", "--unified=0", "--no-color"));
  const lines = new Map<string, Set<number> | undefined>([...added].filter(([path]) => isDocFile(path)));
  const untracked = git("ls-files", "--others", "--exclude-standard", "-z").split("\0").filter(isDocFile);
  return new Map([...lines, ...untracked.map((path): [string, undefined] => [path, undefined])]);
}

const seen = new Set<string>();
for (const [path, only] of targets(process.argv.slice(2))) {
  if (existsSync(path)) {
    for (const { at, rule, text } of findings(path, readFileSync(path, "utf8"), only)) {
      console.log(`${path}:${at} [${rule}] ${text}`);
      seen.add(rule);
    }
  }
}
for (const rule of RULES.filter(({ name }) => seen.has(name))) {
  console.log(`${rule.name}: ${rule.reason}`);
}
console.log(seen.size === 0 ? "Nothing to reread." : "Reread these against /stop-slop.");
