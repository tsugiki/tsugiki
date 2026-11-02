// Rust's coverage gate (ADR 0009): `measure <lcov>` runs the tests on this OS, and `gate <lcov>...` merges
// the OSes' reports and fails below 100% of lines and functions.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { checkReports, relativeLcov } from "./lcov.ts";

// Stable Rust can't exclude a single function, so an exclusion is a whole file.
const EXCLUDED = [
  // coverage: Tauri's start-up needs a real window, and the tauri-driver smoke tests cover it.
  "crates/app/src/startup\\.rs$",
  // coverage: the binary's entry point only calls the start-up code.
  "crates/app/src/main\\.rs$",
  // coverage: the real network and the real gh, which tests replace and release checks exercise.
  "crates/core/src/github/live\\.rs$",
];

const measure = (path: string) => {
  mkdirSync(dirname(path), { recursive: true });
  const ignore = EXCLUDED.join("|").replaceAll("/", "[/\\\\]");
  const args = [
    "llvm-cov",
    "--workspace",
    "--lcov",
    "--output-path",
    path,
    "--ignore-filename-regex",
    ignore,
  ];
  const result = spawnSync("cargo", args, { stdio: "inherit" });
  if (result.error) console.error(result.error.message);
  if (result.status !== 0) process.exit(result.status ?? 1);
  writeFileSync(path, relativeLcov(readFileSync(path, "utf8"), process.cwd()));
};

const gate = (paths: string[]) => {
  const gaps = checkReports(paths.map((path) => ({ path, text: readFileSync(path, "utf8") })));
  if (gaps.length > 0) {
    console.error(`Rust coverage is below 100%:\n${gaps.join("\n")}`);
    process.exit(1);
  }
  console.log(`Rust coverage is 100%, from ${paths.length} report(s).`);
};

const [command, ...paths] = process.argv.slice(2);
if (command === "measure" && paths[0]) measure(paths[0]);
else if (command === "gate" && paths.length > 0) gate(paths);
else {
  console.error("usage: rust-coverage.ts measure <lcov> | gate <lcov>...");
  process.exit(2);
}
