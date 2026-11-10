import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { dependencies, problems } from "./token-crate.ts";

describe("token-crate", () => {
  it("reads each dependency table, whatever the target", () => {
    const manifest = [
      "[package]",
      'name = "tsugiki-token"',
      "[dependencies]",
      "# A comment.",
      "ureq.workspace = true",
      'url = "2"',
      "[target.'cfg(windows)'.dependencies]",
      "windows = { workspace = true }",
      "[dev-dependencies]",
      "insta.workspace = true",
      "[build-dependencies.cc]",
      'version = "1"',
      "[lints]",
      "workspace = true",
    ].join("\n");
    expect(dependencies(manifest)).toEqual(["ureq", "url", "windows", "insta", "cc"]);
  });

  it("counts a renamed dependency by the package it links", () => {
    const manifest = [
      "[dependencies] # The network.",
      'ureq = { package = "evil-http", version = "1" }',
      '"quoted" = "1"',
      "'single' = '1'",
      "[dependencies.url]",
      'package = "other-url"',
      'version = "2"',
      "[dependencies.rustls]",
      'version = "0.23"',
    ].join("\n");
    expect(dependencies(manifest)).toEqual(["evil-http", "quoted", "single", "other-url", "rustls"]);
  });

  it("names ADR 0019 for a crate off the list", () => {
    const found = problems("[dependencies]\nrustls.workspace = true\nlog.workspace = true\n");
    expect(found).toHaveLength(1);
    expect(found[0]).toContain("depends on log");
    expect(found[0]).toContain("ADR 0019");
  });

  it("finds nothing wrong with the real manifest", () => {
    // Vitest runs from `ui/`, as scope.test.ts does.
    const manifest = readFileSync("../crates/token/Cargo.toml", "utf8");
    expect(dependencies(manifest)).toEqual(["rustls", "ureq", "url"]);
    expect(problems(manifest)).toEqual([]);
  });
});
