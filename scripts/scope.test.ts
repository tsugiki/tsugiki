import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { outputs, paths, scope } from "./scope.ts";

/** Each file under `folder` whose name ends with `.rs`. */
function rustFiles(folder: string): string[] {
  return readdirSync(folder, { recursive: true, encoding: "utf8" })
    .filter((name) => name.endsWith(".rs"))
    .map((name) => join(folder, name));
}

describe("scope", () => {
  it.each([
    [["docs/scope.md"], "docs"],
    [["README.md", "ui/NOTES.md", "docs/adr/0009.txt"], "docs"],
    [["ui/src/App.svelte"], "ui"],
    [["ui/src/App.svelte", "docs/build-order.md", "ui/README.md"], "ui"],
    [["ui/vite.config.ts", "ui/e2e/layout.test.ts"], "ui"],
  ])("reads %j as %s", (paths, expected) => {
    expect(scope(paths)).toBe(expected);
  });

  it.each([
    [["ui/src/App.svelte", "ui/src/lib/data/bindings.ts"]],
    [["ui/package.json"]],
    [["ui/src/App.svelte", "package-lock.json"]],
    [["ui/src/App.svelte", ".github/workflows/ci.yml"]],
    [["scripts/scope.ts"]],
    [["crates/core/src/lib.rs"]],
    [["uix/a.ts"]],
    [["crates/core/src/highlight/samples/markdown.md"]],
    [["docs/schema.md"]],
    [["ui/src/App.svelte", "docs/schema.md"]],
  ])("runs everything for %j", (given) => {
    expect(scope(given)).toBe("all");
  });

  it("runs everything when it can't see a path", () => {
    expect(scope([])).toBe("all");
  });

  // A Rust file that names a path under ui/ may read it, and then a change to that file needs the Rust
  // checks. The highlighter's test names one only to pick its language.
  it("runs everything for each file under ui/ that the Rust code names", () => {
    const named = new Set(
      rustFiles("../crates").flatMap((file) =>
        [...readFileSync(file, "utf8").matchAll(/"(?:\.\.\/)*(ui\/[^"]+)"/g)].map((match) => match[1]),
      ),
    );
    named.delete("ui/src/App.svelte");
    expect(named.size).toBeGreaterThan(0);
    for (const path of named) {
      expect(scope([path ?? ""]), path).toBe("all");
    }
  });
});

describe("paths", () => {
  it("reads one path a line and drops the blank ones", () => {
    expect(paths("ui/a.ts\r\ndocs/b.md\n\n")).toEqual(["ui/a.ts", "docs/b.md"]);
  });
});

describe("outputs", () => {
  it.each([
    ["docs", "docs-only=true\nui-only=true\n"],
    ["ui", "docs-only=false\nui-only=true\n"],
    ["all", "docs-only=false\nui-only=false\n"],
  ] as const)("gives CI's outputs for %s", (given, expected) => {
    expect(outputs(given)).toBe(expected);
  });
});
