// `just lint` runs this: fails when `crates/token/Cargo.toml` names a dependency outside its list
// (ADR 0019). The rule is in token-crate.ts.
import { readFileSync } from "node:fs";

import { ALLOWED, problems } from "./token-crate.ts";

const found = problems(readFileSync("crates/token/Cargo.toml", "utf8"));
for (const problem of found) {
  console.error(problem);
}
if (found.length > 0) {
  process.exit(1);
}
console.log(`crates/token depends only on ${ALLOWED.join(", ")}.`);
