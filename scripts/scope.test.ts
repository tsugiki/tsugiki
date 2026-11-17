import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { isVersionBump, outputs, paths, scope } from "./scope.ts";

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

  // A Rust file that names a path under ui/ or docs/ may read it, and then a change to that file needs the
  // Rust checks. The highlighter's test names one only to pick its language.
  it("runs everything for each file under ui/ or docs/ that the Rust code names", () => {
    const named = new Set(
      rustFiles("../crates").flatMap((file) =>
        [...readFileSync(file, "utf8").matchAll(/"(?:\.\.\/)*((?:ui\/[^"]+)|(?:docs\/[\w./-]+))"/g)].map(
          (match) => match[1],
        ),
      ),
    );
    named.delete("ui/src/App.svelte");
    expect(named.has("docs/devel/keyboard.md") && named.has("ui/src/lib/data/keymap.ts")).toBe(true);
    for (const path of named) {
      expect(scope([path ?? ""]), path).toBe("all");
    }
  });
});

/** A `git diff -U0` of `file` that moves its version line, `times` over, as in each crate of Cargo.lock. */
function bump(file: string, from: string, to: string, times = 1): string {
  const hunk = `@@ -6 +6 @@\n-version = "${from}"\n+version = "${to}"\n`;
  return `diff --git a/${file} b/${file}\n--- a/${file}\n+++ b/${file}\n${hunk.repeat(times)}`;
}

const SETS_VERSION = bump("Cargo.lock", "0.3.2", "0.3.3", 6) + bump("Cargo.toml", "0.3.2", "0.3.3");

describe("a release PR", () => {
  it.each([[["CHANGELOG.md", "Cargo.lock", "Cargo.toml"]], [["Cargo.lock", "Cargo.toml"]]])(
    "that only sets the version in %j skips the builds and tests",
    (given) => {
      expect(scope(given, SETS_VERSION)).toBe("release");
    },
  );

  it.each([
    ["a dependency's bump", `${SETS_VERSION}@@ -900,2 +900,2 @@\n-version = "1.0.1"\n+version = "1.0.2"\n`],
    ["a new checksum", `${SETS_VERSION}@@ -901 +901 @@\n-checksum = "aa"\n+checksum = "bb"\n`],
    ["a line it only adds", `${SETS_VERSION}@@ -0,0 +1 @@\n+version = "0.3.3"\n`],
    ["no change of version", bump("Cargo.toml", "0.3.3", "0.3.3")],
    ["nothing it can read", ""],
  ])("runs everything for %s", (_, cargoDiff) => {
    expect(scope(["CHANGELOG.md", "Cargo.lock", "Cargo.toml"], cargoDiff)).toBe("all");
  });

  it.each([
    [["Cargo.lock"]],
    [["CHANGELOG.md", "Cargo.lock", "Cargo.toml", "crates/core/src/lib.rs"]],
    [["CHANGELOG.md", "Cargo.lock", "Cargo.toml", ".github/workflows/release.yml"]],
    [["Cargo.toml", "ui/src/App.svelte"]],
  ])("runs everything when it also changes %j", (given) => {
    expect(scope(given, SETS_VERSION)).toBe("all");
  });

  it("reads the notes alone as docs", () => {
    expect(scope(["CHANGELOG.md"], "")).toBe("docs");
  });
});

describe("isVersionBump", () => {
  it("reads a diff with Windows line ends", () => {
    expect(isVersionBump(SETS_VERSION.replaceAll("\n", "\r\n"))).toBe(true);
  });

  it("refuses two versions moving at once", () => {
    expect(isVersionBump(bump("Cargo.toml", "0.3.2", "0.3.3") + bump("Cargo.lock", "0.3.1", "0.3.3"))).toBe(
      false,
    );
  });
});

describe("paths", () => {
  it("reads one path a line and drops the blank ones", () => {
    expect(paths("ui/a.ts\r\ndocs/b.md\n\n")).toEqual(["ui/a.ts", "docs/b.md"]);
  });
});

describe("outputs", () => {
  it.each([
    ["docs", "docs-only=true\nui-only=true\nrelease-only=false\n"],
    ["ui", "docs-only=false\nui-only=true\nrelease-only=false\n"],
    ["release", "docs-only=false\nui-only=false\nrelease-only=true\n"],
    ["all", "docs-only=false\nui-only=false\nrelease-only=false\n"],
  ] as const)("gives CI's outputs for %s", (given, expected) => {
    expect(outputs(given)).toBe(expected);
  });
});
