import { describe, expect, it } from "vitest";

import { notesFor, versionOfTag } from "./release-notes.ts";

const CARGO_TOML = `[workspace]
members = ["crates/core"]

[workspace.package]
edition = "2024"
version = "0.2.0"

[workspace.dependencies]
serde = { version = "1.0.0" }
`;

const CHANGELOG = `# Changelog

## Unreleased

### Added

- Something not released yet.

## 0.2.0

### Added

- The second thing.

### Fixed

- A bug.

## 0.1.0

### Added

- The first thing.
`;

describe("versionOfTag", () => {
  it("gives the version when the tag names the workspace's version", () => {
    expect(versionOfTag("v0.2.0", CARGO_TOML)).toBe("0.2.0");
  });

  it("fails when the tag names another version, because the installers would carry Cargo.toml's", () => {
    expect(() => versionOfTag("v0.3.0", CARGO_TOML)).toThrow(
      "The tag v0.3.0 doesn't match version 0.2.0 in Cargo.toml.",
    );
  });

  it("fails when Cargo.toml doesn't say the workspace's version", () => {
    expect(() => versionOfTag("v1.0.0", '[workspace.dependencies]\nserde = "1.0.0"\n')).toThrow(
      "Cargo.toml has no version in [workspace.package].",
    );
  });

  it("doesn't take another section's version for the workspace's", () => {
    expect(() => versionOfTag("v1.0.0", '[package]\nversion = "1.0.0"\n\n[workspace.package]\n')).toThrow(
      "Cargo.toml has no version in [workspace.package].",
    );
  });
});

describe("notesFor", () => {
  it("gives the version's section, up to the next version", () => {
    expect(notesFor(CHANGELOG, "0.2.0")).toBe("### Added\n\n- The second thing.\n\n### Fixed\n\n- A bug.");
  });

  it("gives the last section, which has no version after it", () => {
    expect(notesFor(CHANGELOG, "0.1.0")).toBe("### Added\n\n- The first thing.");
  });

  it("fails when the changelog has no section for the version", () => {
    expect(() => notesFor(CHANGELOG, "0.3.0")).toThrow("CHANGELOG.md has no section for 0.3.0");
  });

  it("doesn't take a section whose heading only starts with the version", () => {
    expect(() => notesFor("## 0.1.01\n\n- Later.\n", "0.1.0")).toThrow("no section for 0.1.0");
  });

  it("fails when the version's section is empty", () => {
    expect(() => notesFor("## 0.2.0\n\n## 0.1.0\n\n- First.\n", "0.2.0")).toThrow(
      "section for 0.2.0 is empty",
    );
  });
});
