// Rust's coverage gate (ADR 0009): runs the tests on this OS and fails below 100% of lines and functions.
// It leaves the report at target/coverage/lcov.info, for Codecov.
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";

// Stable Rust can't exclude a single function, so an exclusion is a whole file.
const EXCLUDED = [
  // coverage: Tauri's start-up needs a real window, and the smoke tests cover it.
  "crates/app/src/startup\\.rs$",
  // coverage: the binary's entry point only calls the start-up code.
  "crates/app/src/main\\.rs$",
  // coverage: the real network and the real gh, which tests replace and release checks exercise.
  "crates/core/src/github/live\\.rs$",
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
const result = spawnSync("cargo", args, { stdio: "inherit" });
if (result.error) console.error(result.error.message);
if (result.status !== 0) {
  // A closure that never ran can sit on a covered line, and then cargo-llvm-cov fails without saying why.
  console.error("A test failed, or coverage is below 100%. `cargo llvm-cov report --html` shows any gap.");
}
process.exit(result.status ?? 1);
