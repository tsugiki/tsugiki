// Regenerates the UI's TypeScript bindings from the Rust commands (ADR 0008).
// `cargo test` fails while the committed bindings are out of date; this rewrites them.
import { spawnSync } from "node:child_process";

const result = spawnSync(
  "cargo",
  ["test", "--quiet", "-p", "tsugiki-app", "--lib", "bindings_are_up_to_date"],
  { stdio: "inherit", env: { ...process.env, TSUGIKI_UPDATE_BINDINGS: "1" } },
);
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
