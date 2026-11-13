// Rust's coverage gate (ADR 0009): runs the tests on this OS and fails below 100% of lines and functions.
// It leaves the report at target/coverage/lcov.info, for Codecov.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync } from "node:fs";

import { filesWithGaps, hasStaleBuilds, unrunRegions } from "./coverage-gaps.ts";

// Stable Rust can't exclude a single function, so an exclusion is a whole file.
const EXCLUDED = [
  // coverage: Tauri's start-up needs a real window, and the smoke tests cover it.
  "crates/app/src/startup\\.rs$",
  // coverage: the binary's entry point only calls the start-up code.
  "crates/app/src/main\\.rs$",
  // coverage: thin async commands. The report calls the block Tauri's macro writes unrun, and IPC tests
  // run it.
  "crates/app/src/slow_commands\\.rs$",
  // coverage: the real network and the real gh, which tests replace and release checks exercise (ADR 0019).
  "crates/token/src/live\\.rs$",
  // coverage: the real git, which tests run, and the hand-off to the token crate's client, which they
  // replace.
  "crates/core/src/github/live\\.rs$",
  // coverage: starting a real language server, which tests replace with a fake one over in-memory pipes.
  "crates/lsp/src/process\\.rs$",
  // coverage: one call on a real webview, which `npm run smoke` shows working.
  "crates/webview-keys/src/lib\\.rs$",
];

mkdirSync("target/coverage", { recursive: true });
const ignore = EXCLUDED.join("|").replaceAll("/", "[/\\\\]");
const args = [
  "llvm-cov",
  "--workspace",
  "--lcov",
  "--output-path",
  "target/coverage/lcov.info",
  "--ignore-filename-regex",
  ignore,
  "--fail-under-lines",
  "100",
  "--fail-under-functions",
  "100",
  "--show-missing-lines",
];

/** Says what never ran. A closure or a `?` can sit on a covered line, and then cargo-llvm-cov doesn't say. */
function printGaps(): void {
  const lcovPath = "target/coverage/lcov.info";
  const report = "target/coverage/report.txt";
  const made = spawnSync("cargo", [
    "llvm-cov",
    "report",
    "--text",
    "--output-path",
    report,
    "--ignore-filename-regex",
    ignore,
  ]);
  if (made.status !== 0) {
    console.error("`cargo llvm-cov report` failed, so these are counts with no source lines.");
  }
  const text = made.status === 0 ? readFileSync(report, "utf8") : "";
  for (const gap of filesWithGaps(readFileSync(lcovPath, "utf8"))) {
    console.error(`${gap.path}: lines ${gap.lines}, functions ${gap.functions}`);
    for (const { at, source, whole } of unrunRegions(text, gap.path)) {
      console.error(`  ${at}: ${whole ? "never ran" : "part never ran"}: ${source}`);
    }
  }
}

const DEPS = "target/llvm-cov-target/debug/deps";

/** Says whether a build made under another toolchain, `Cargo.lock` or set of flags sits beside this one. */
function staleBuildsThere(): boolean {
  return existsSync(DEPS) && hasStaleBuilds(readdirSync(DEPS));
}

/**
 * Runs the tests and the gate. `cargo llvm-cov` cleans the build first, and recompiling takes about 30
 * seconds, so `keep` drops only the last run's line counts and keeps the instrumented build.
 */
function cover(keep: boolean): number {
  if (keep) {
    const cleaned = spawnSync("cargo", ["llvm-cov", "clean", "--profraw-only"], { stdio: "inherit" });
    if (cleaned.status !== 0) return cleaned.status ?? 1;
  }
  const result = spawnSync("cargo", keep ? [...args, "--no-clean"] : args, { stdio: "inherit" });
  if (result.error) console.error(result.error.message);
  return result.status ?? 1;
}

// Locally the build stays between runs. In #252's runs that neither hid a gap nor added a false one, as long
// as no build from another configuration was there. One with no line counts can only add false gaps, so a
// failed run with one there runs again clean. CI always cleans, as the backstop.
const keep = process.env.CI !== "true" && !staleBuildsThere();
let started = Date.now();
let status = cover(keep);
if (status !== 0 && keep && staleBuildsThere()) {
  console.error(
    "A build from another toolchain, Cargo.lock or set of flags was in the report. Running clean.",
  );
  started = Date.now();
  status = cover(false);
}
if (status !== 0) {
  // A failed test leaves the last run's report in place, so only a fresh report says anything about now.
  const fresh = statSync("target/coverage/lcov.info", { throwIfNoEntry: false });
  if (fresh !== undefined && fresh.mtimeMs >= started) {
    printGaps();
    console.error("Coverage is below 100%. A `?` or a closure alone on a line needs its own covered line.");
  } else {
    console.error("A test failed, so there is no new coverage report.");
  }
}
process.exit(status);
