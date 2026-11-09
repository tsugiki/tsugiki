import { describe, expect, it } from "vitest";

import { outputs, scope } from "./scope.ts";

describe("scope", () => {
  it.each([
    [["docs/scope.md"], "docs"],
    [["README.md", "crates/core/NOTES.md", "docs/adr/0009.txt"], "docs"],
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
  ])("runs everything for %j", (paths) => {
    expect(scope(paths)).toBe("all");
  });

  it("runs everything when it can't see a path", () => {
    expect(scope([])).toBe("all");
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
