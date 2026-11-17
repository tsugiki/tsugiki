import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { EVERY_JOB, isVersionBump, type Job, jobs, narrowed, outputs, paths, readTested } from "./scope.ts";

/** The Cargo diff of a change that can't be a release PR, which `jobs` mustn't read. */
function unread(): string {
  throw new Error("jobs read the Cargo diff of a change that can't set the version");
}

const RUST: Job[] = ["rust", "dependencies", "rust-coverage", "macos-rust"];
const UI: Job[] = ["ui", "playwright", "macos-ui"];

/** Each file under `folder` whose name ends with one of `endings`. */
function filesEnding(folder: string, ...endings: string[]): string[] {
  return readdirSync(folder, { recursive: true, encoding: "utf8" })
    .filter((name) => endings.some((ending) => name.endsWith(ending)))
    .map((name) => join(folder, name));
}

describe("jobs", () => {
  it.each([
    [["docs/scope.md"], []],
    [["README.md", "ui/NOTES.md", "docs/adr/0009.txt"], []],
    [["README.md", "docs/adr/0009.txt", "CHANGELOG.md"], []],
    [["ui/src/App.svelte"], UI],
    [["ui/src/App.svelte", "docs/build-order.md", "ui/README.md"], UI],
    [["ui/vite.config.ts", "ui/e2e/layout.test.ts"], UI],
    [["crates/core/src/lib.rs"], RUST],
    [["crates/core/src/lib.rs", "docs/devel/sync.md"], RUST],
    [["crates/core/src/highlight/samples/markdown.md"], RUST],
    [["Cargo.lock"], RUST],
    [["rust-toolchain.toml", "clippy.toml", "deny.toml"], RUST],
    [["docs/schema.md"], RUST],
    [["ui/src/lib/data/bindings.ts"], [...RUST, ...UI]],
    [["ui/package.json"], [...RUST, ...UI]],
    [["crates/app/tauri.conf.json"], [...RUST, ...UI]],
    [["crates/token/Cargo.toml"], [...RUST, ...UI]],
    [
      ["crates/core/src/lib.rs", "ui/src/App.svelte"],
      [...RUST, ...UI],
    ],
  ])("gives %j the jobs %j", (given, expected) => {
    expect(jobs(given, unread)).toEqual(expected);
  });

  it.each([
    [["ui/src/App.svelte", "package-lock.json"]],
    [["ui/src/App.svelte", ".github/workflows/ci.yml"]],
    [["scripts/scope.ts"]],
    [["justfile"]],
    [["uix/a.ts"]],
    [["prototype/src/App.svelte"]],
  ])("runs everything for %j", (given) => {
    expect(jobs(given, unread)).toEqual(EVERY_JOB);
  });

  it("runs everything when it can't see a path", () => {
    expect(jobs([], unread)).toEqual(EVERY_JOB);
  });

  // A Rust file that names a path under ui/ or docs/ may read it, and then a change to that file needs the
  // Rust jobs. The highlighter's test names one only to pick its language.
  it("runs the Rust jobs for each file under ui/ or docs/ that the Rust code names", () => {
    const named = new Set(
      filesEnding("../crates", ".rs").flatMap((file) =>
        [...readFileSync(file, "utf8").matchAll(/"(?:\.\.\/)*((?:ui\/[^"]+)|(?:docs\/[\w./-]+))"/g)].map(
          (match) => match[1],
        ),
      ),
    );
    named.delete("ui/src/App.svelte");
    expect(named.has("docs/devel/keyboard.md") && named.has("ui/src/lib/data/keymap.ts")).toBe(true);
    for (const path of named) {
      expect(jobs([path ?? ""], unread), path).toEqual(expect.arrayContaining(RUST));
    }
  });

  // A UI file that reads a file under crates/ needs the UI jobs to run when that file changes.
  it("runs the UI jobs for each file under crates/ that the UI code reads", () => {
    const read = new Set(
      [...filesEnding("src", ".ts", ".svelte"), ...filesEnding("e2e", ".ts")].flatMap((file) =>
        [...readFileSync(file, "utf8").matchAll(/"\.\.\/(crates\/[\w./-]+)"/g)].map((match) => match[1]),
      ),
    );
    expect(read.has("crates/app/tauri.conf.json")).toBe(true);
    for (const path of read) {
      expect(jobs([path ?? ""], unread), path).toEqual(expect.arrayContaining(UI));
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
    "that only sets the version in %j runs only cargo-deny and cargo-machete",
    (given) => {
      expect(jobs(given, () => SETS_VERSION)).toEqual(["dependencies"]);
    },
  );

  it.each([
    ["a dependency's bump", `${SETS_VERSION}@@ -900,2 +900,2 @@\n-version = "1.0.1"\n+version = "1.0.2"\n`],
    ["a new checksum", `${SETS_VERSION}@@ -901 +901 @@\n-checksum = "aa"\n+checksum = "bb"\n`],
    ["a line it only adds", `${SETS_VERSION}@@ -0,0 +1 @@\n+version = "0.3.3"\n`],
    ["no change of version", bump("Cargo.toml", "0.3.3", "0.3.3")],
    ["nothing it can read", ""],
  ])("runs the Rust jobs for %s", (_, cargoDiff) => {
    expect(jobs(["CHANGELOG.md", "Cargo.lock", "Cargo.toml"], () => cargoDiff)).toEqual(RUST);
  });

  it.each([
    [["CHANGELOG.md", "Cargo.toml"], RUST],
    [["CHANGELOG.md", "Cargo.lock", "Cargo.toml", "crates/core/src/lib.rs"], RUST],
    [
      ["CHANGELOG.md", "Cargo.lock", "Cargo.toml", "ui/src/App.svelte"],
      [...RUST, ...UI],
    ],
    [["CHANGELOG.md", "Cargo.lock", "Cargo.toml", ".github/workflows/release.yml"], EVERY_JOB],
  ])("reads %j as more than a version", (given, expected) => {
    expect(jobs(given, unread)).toEqual(expected);
  });
});

describe("isVersionBump", () => {
  it("reads a diff with Windows line ends", () => {
    expect(isVersionBump(SETS_VERSION.replaceAll("\n", "\r\n"))).toBe(true);
  });

  it("reads the headers of any prefix git's settings give", () => {
    const mnemonic = SETS_VERSION.replaceAll("--- a/", "--- c/").replaceAll("+++ b/", "+++ w/");
    expect(isVersionBump(mnemonic)).toBe(true);
  });

  it.each([
    ["from", bump("Cargo.toml", "0.3.2", "0.3.3") + bump("Cargo.lock", "0.3.1", "0.3.3")],
    ["to", bump("Cargo.toml", "0.3.2", "0.3.3") + bump("Cargo.lock", "0.3.2", "0.3.4")],
  ])("refuses two versions to move %s", (_, diff) => {
    expect(isVersionBump(diff)).toBe(false);
  });
});

const HEAD = "a".repeat(40);
const BASE = "b".repeat(40);

describe("readTested", () => {
  it("reads a run's head and base", () => {
    expect(readTested(JSON.stringify({ head: HEAD, base: BASE }))).toEqual({ head: HEAD, base: BASE });
  });

  it.each([
    ["not json"],
    ["null"],
    ["7"],
    [JSON.stringify({ head: HEAD })],
    [JSON.stringify({ head: "main", base: BASE })],
    [JSON.stringify({ head: HEAD, base: 7 })],
  ])("refuses %s", (text) => {
    expect(readTested(text)).toBeUndefined();
  });
});

describe("narrowed", () => {
  it("keeps the jobs whose inputs changed since the run that passed", () => {
    expect(narrowed([...RUST, ...UI], ["ui", "playwright", "macos-ui"])).toEqual(UI);
  });

  it("keeps none for a change since that run that touched only docs", () => {
    expect(narrowed(EVERY_JOB, [])).toEqual([]);
  });

  it("keeps every job the PR needs when no run passed", () => {
    expect(narrowed(["dependencies"], undefined)).toEqual(["dependencies"]);
  });

  it("never adds a job the PR doesn't need against its base", () => {
    expect(narrowed(["ui"], EVERY_JOB)).toEqual(["ui"]);
  });
});

describe("paths", () => {
  it("reads one path a line and drops the blank ones", () => {
    expect(paths("ui/a.ts\r\ndocs/b.md\n\n")).toEqual(["ui/a.ts", "docs/b.md"]);
  });
});

describe("outputs", () => {
  it("writes the jobs as a JSON list", () => {
    expect(outputs(["rust", "macos-rust"])).toBe('jobs=["rust","macos-rust"]\n');
  });

  it("writes an empty list for a change that needs none", () => {
    expect(outputs([])).toBe("jobs=[]\n");
  });
});
