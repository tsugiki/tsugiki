// The rules of `just dupes`: the scans jscpd runs over the code, and the repeats AGENTS.md's "One copy" rule
// lets stay. The gate counts a copy between two files. A repeat inside one file sits in one reader's view,
// and tests repeat their steps from test to test.
import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { join, relative, sep } from "node:path";

/** One run of jscpd over some folders, in the formats it names, with the smallest copy it counts. */
export interface Scan {
  name: string;
  paths: readonly string[];
  formats: readonly string[];
  /** Fewer tokens than this, in jscpd's weak mode, which skips comments, isn't a copy. */
  minTokens: number;
  /** Globs of files left out, relative to the root. */
  ignore: readonly string[];
}

/**
 * Each scan. jscpd splits a Svelte file into its script, markup and style, and compares each with the same
 * kind of code in other files. A Rust line holds more tokens than a TypeScript one: five lines of a Rust
 * helper come to about 65 tokens, and five of a TypeScript one to about 50, so each language gets its own
 * size.
 */
export const SCANS: readonly Scan[] = [
  {
    name: "Rust",
    paths: ["crates"],
    formats: ["rust"],
    minTokens: 60,
    // The highlighter's samples are code in each language it reads, written to be highlighted.
    ignore: ["crates/core/src/highlight/samples/**"],
  },
  {
    name: "UI and scripts",
    paths: ["ui/src", "ui/e2e", "scripts"],
    formats: ["typescript", "svelte"],
    minTokens: 50,
    // Generated from the Rust commands and the core's key table.
    ignore: ["ui/src/lib/data/bindings.ts", "ui/src/lib/data/keymap.ts"],
  },
];

/** The lines of one file a copy spans. */
export interface Place {
  /** Relative to the root, with forward slashes. */
  file: string;
  start: number;
  end: number;
}

/** A block of code jscpd found in two places. */
export interface Copy {
  tokens: number;
  first: Place;
  second: Place;
}

/**
 * Copies between two files that stay, each named by its size, and why they stay. The size ties an entry to
 * the blocks it was written for: a new copy between the same files, or an edit to an allowed one, changes the
 * sizes jscpd reports, and the gate fails until someone looks at it again.
 */
export interface Allowed {
  files: readonly [string, string];
  /** The size in tokens of each copy allowed between the two files. */
  tokens: readonly number[];
  why: string;
}

/** The repeats that stay. The gate fails on a size it no longer finds, so the list doesn't outlive them. */
export const ALLOWED: readonly Allowed[] = [
  {
    files: ["crates/core/benches/highlight.rs", "crates/core/src/highlight/tests.rs"],
    tokens: [88],
    why: "The benchmark times the C the test highlights, and a benchmark can't reach `cfg(test)` code (#831).",
  },
  {
    files: ["crates/core/src/mirror/tests.rs", "crates/core/src/mirror/tests/upkeep.rs"],
    tokens: [63],
    why: "Two tests send the mirror worker the same fetches, and check different outcomes of them.",
  },
  {
    files: ["crates/core/src/mirror/tests/states.rs", "crates/core/src/mirror/tests/upkeep.rs"],
    tokens: [61],
    why: "Two tests send the mirror worker the same clone and fetch, and check different outcomes of them.",
  },
  {
    files: ["crates/core/src/store/issues.rs", "crates/core/src/store/pull_requests.rs"],
    tokens: [82],
    why: "`read_titled` and `read_all` read different tables into the same shape (#831).",
  },
  {
    files: ["crates/core/src/store/issues.rs", "crates/core/src/store/pr_links.rs"],
    tokens: [60],
    why: "Two queries step through their rows as rusqlite does, and build different values from them.",
  },
  {
    files: ["crates/core/src/sync/batches/tests.rs", "crates/core/src/sync/searches/tests.rs"],
    tokens: [64, 89],
    why: "Tests run a first sync the same way, and each checks the requests of its own phase.",
  },
  {
    files: ["crates/core/src/sync/origins/tests.rs", "crates/core/src/sync/trees/tests.rs"],
    tokens: [64, 80],
    why: "Tests run a sync, or two in a row, the same way, and each checks its own phase.",
  },
  {
    files: ["ui/src/lib/state/pr-files.svelte.test.ts", "ui/src/lib/ui/FilesTab.test.ts"],
    tokens: [71],
    why: "Each test file mocks the commands its subject calls, which #831 keeps in the file.",
  },
  {
    files: ["ui/src/lib/ui/FilesTab.test.ts", "ui/src/lib/ui/PrDetail.test.ts"],
    tokens: [78],
    why: "Each test file mocks the commands its subject calls, which #831 keeps in the file.",
  },
  {
    files: ["ui/src/lib/ui/RepositoryHome.test.ts", "ui/src/routes/page.test.ts"],
    tokens: [50],
    why: "Each test file mocks the commands its subject calls, which #831 keeps in the file.",
  },
];

const jscpd = createRequire(import.meta.url).resolve("jscpd/run-jscpd.js");

/**
 * The copies `scan` finds in the checkout at `root`, run with Node from jscpd's npm package. jscpd writes its
 * report into the folder `output`, and reads `.gitignore` to leave out what git ignores.
 */
export function find(scan: Scan, root: string, output: string): Copy[] {
  const args = [
    ...scan.paths,
    ["--format", scan.formats.join(",")],
    ["--ignore", scan.ignore.join(",")],
    ["--min-tokens", String(scan.minTokens)],
    // The tokens alone decide: five lines of Rust can hold fewer lines than jscpd's default of five.
    ["--min-lines", "1"],
    ["--mode", "weak"],
    ["--reporters", "json"],
    ["--output", output],
    ["--absolute", "--silent", "--no-tips"],
  ].flat();
  execFileSync(process.execPath, [jscpd, ...args], { cwd: root, stdio: ["ignore", "ignore", "inherit"] });
  // jscpd names each file by its real path, as macOS's temp folder, a link into /private, doesn't.
  return readReport(readFileSync(join(output, "jscpd-report.json"), "utf8"), realpathSync.native(root));
}

interface ReportedPlace {
  name: string;
  start: number;
  end: number;
}

/**
 * The copies in what jscpd's JSON reporter wrote, each place named relative to `root`. On Windows jscpd
 * starts each absolute path with `\\?\`, which `root` lacks.
 */
export function readReport(text: string, root: string): Copy[] {
  const { duplicates } = JSON.parse(text) as {
    duplicates?: { tokens: number; firstFile: ReportedPlace; secondFile: ReportedPlace }[];
  };
  if (duplicates === undefined) {
    throw new Error("jscpd's report has no list of duplicates");
  }
  const place = ({ name, start, end }: ReportedPlace): Place => ({
    file: relative(root, name.replace(/^\\\\\?\\/, ""))
      .split(sep)
      .join("/"),
    start,
    end,
  });
  return duplicates.map(({ tokens, firstFile, secondFile }) => ({
    tokens,
    first: place(firstFile),
    second: place(secondFile),
  }));
}

/** One size an entry of `ALLOWED` allows between its two files. */
export interface AllowedCopy {
  files: readonly [string, string];
  tokens: number;
}

/** Whether `allowed` is a copy of `copy`'s size between its two files, in either order. */
const allows = ({ files: [one, two], tokens }: AllowedCopy, { first, second, ...copy }: Copy): boolean =>
  tokens === copy.tokens &&
  ((first.file === one && second.file === two) || (first.file === two && second.file === one));

/**
 * The copies that fail the gate: those between two files that `allowed` doesn't name with their size.
 * `unused` lists each size `allowed` names that matches no copy, which the gate fails on too.
 */
export function judge(
  found: readonly Copy[],
  allowed: readonly Allowed[],
): { copies: Copy[]; unused: AllowedCopy[] } {
  const between = found.filter(({ first, second }) => first.file !== second.file);
  const sizes = allowed.flatMap(({ files, tokens }) => tokens.map((size) => ({ files, tokens: size })));
  return {
    copies: between.filter((copy) => !sizes.some((size) => allows(size, copy))),
    unused: sizes.filter((size) => !between.some((copy) => allows(size, copy))),
  };
}

/** A line that names both places of `copy`, for the gate's log. */
export function describeCopy({ tokens, first, second }: Copy): string {
  const at = ({ file, start, end }: Place) => `${file}:${String(start)}-${String(end)}`;
  return `${at(first)} and ${at(second)} hold the same ${String(tokens)} tokens`;
}
