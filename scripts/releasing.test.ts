import { describe, expect, it } from "vitest";

import {
  type Tree,
  isoDate,
  prepareRefusal,
  releaseVersion,
  shipRefusal,
  tagRefusal,
  withReleasedWip,
  withVersion,
} from "./releasing.ts";

const CARGO_TOML = `[workspace]
members = ["crates/core"]

[workspace.package]
edition = "2024"
version = "0.2.3"
rust-version = "1.99"

[workspace.dependencies]
serde = { version = "1.0.0" }
`;

const CHANGELOG = `# Changelog

Release notes for Tsugiki.

## WIP

### Added

- Something new.

## Version 0.2.3 (2026-11-12)

- The last release.
`;

const CURRENT: Tree = { branch: "main", dirty: false, head: "abc", originMain: "abc" };

describe("releaseVersion", () => {
  it.each(["0.2.4", "1.0.0", "10.20.30"])("takes %j", (arg) => {
    expect(releaseVersion(arg)).toBe(arg);
  });

  it.each([undefined, "", "v0.2.4", "0.2", "0.2.4.1", "0.02.4", "0.2.4-rc1", " 0.2.4"])(
    "refuses %j",
    (arg) => {
      expect(releaseVersion(arg)).toBeUndefined();
    },
  );
});

describe("withVersion", () => {
  it("sets the workspace's version and nothing else", () => {
    expect(withVersion(CARGO_TOML, "0.2.4")).toBe(
      CARGO_TOML.replace('version = "0.2.3"', 'version = "0.2.4"'),
    );
  });

  it("finds the version in a [workspace.package] that ends the file", () => {
    expect(withVersion('[workspace.package]\nversion = "0.2.3"\n', "0.2.4")).toBe(
      '[workspace.package]\nversion = "0.2.4"\n',
    );
  });

  it("fails without a workspace version", () => {
    expect(() => withVersion('[package]\nversion = "0.1.0"\n', "0.2.4")).toThrow("no version");
  });
});

describe("withReleasedWip", () => {
  it("names WIP's notes after the version and starts an empty WIP above them", () => {
    expect(withReleasedWip(CHANGELOG, "0.2.4", "2026-11-20")).toBe(
      CHANGELOG.replace("## WIP\n", "## WIP\n\n## Version 0.2.4 (2026-11-20)\n"),
    );
  });

  it("fails without a WIP section", () => {
    expect(() => withReleasedWip("# Changelog\n", "0.2.4", "2026-11-20")).toThrow('no section "## WIP"');
  });

  it("fails with two WIP sections", () => {
    expect(() => withReleasedWip(`${CHANGELOG}\n## WIP\n`, "0.2.4", "2026-11-20")).toThrow("2 sections");
  });

  it("fails when the version has a section already", () => {
    expect(() => withReleasedWip(CHANGELOG, "0.2.3", "2026-11-20")).toThrow("already");
  });
});

describe("isoDate", () => {
  it("gives the local date with padded parts", () => {
    expect(isoDate(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
  });
});

describe("prepareRefusal", () => {
  it("lets a later version start from a clean, current main", () => {
    expect(prepareRefusal(CURRENT, "0.2.4", "0.2.3")).toBeUndefined();
    expect(prepareRefusal(CURRENT, "0.10.0", "0.9.9")).toBeUndefined();
    expect(prepareRefusal(CURRENT, "0.2.1", "0.2")).toBeUndefined();
  });

  it.each([
    [CURRENT, undefined, "Usage"],
    [{ ...CURRENT, branch: "" }, "0.2.4", "detached HEAD"],
    [{ ...CURRENT, branch: "list-look" }, "0.2.4", "main"],
    [{ ...CURRENT, dirty: true }, "0.2.4", "Commit"],
    [{ ...CURRENT, originMain: "def" }, "0.2.4", "origin/main"],
    [CURRENT, "0.2.3", "later than 0.2.3"],
    [CURRENT, "0.1.9", "later than 0.2.3"],
  ])("refuses %j with %j", (tree, version, words) => {
    expect(prepareRefusal(tree, version, "0.2.3")).toContain(words);
  });
});

describe("shipRefusal", () => {
  it("lets release/X.Y.Z push when Cargo.toml says X.Y.Z and CHANGELOG.md has its notes", () => {
    const changelog = withReleasedWip(CHANGELOG, "0.2.4", "2026-11-20");
    expect(shipRefusal("release/0.2.4", withVersion(CARGO_TOML, "0.2.4"), changelog)).toBeUndefined();
  });

  it("refuses a branch named for another version", () => {
    expect(shipRefusal("release/0.2.4", CARGO_TOML, CHANGELOG)).toContain("release/0.2.3");
  });

  it("refuses when the release notes fail the check", () => {
    const emptyWip = CHANGELOG.replace("### Added\n\n- Something new.\n\n", "");
    const changelog = withReleasedWip(emptyWip, "0.2.4", "2026-11-20");
    expect(shipRefusal("release/0.2.4", withVersion(CARGO_TOML, "0.2.4"), changelog)).toContain(
      "release-check",
    );
  });
});

describe("tagRefusal", () => {
  const nowhere = { here: false, onGitHub: false };

  it("lets a new tag go on a clean, current main", () => {
    expect(tagRefusal(CURRENT, "v0.2.4", nowhere)).toBeUndefined();
  });

  it.each([
    [{ ...CURRENT, branch: "release/0.2.4" }, nowhere, "main"],
    [{ ...CURRENT, dirty: true }, nowhere, "Commit"],
    [{ ...CURRENT, head: "def" }, nowhere, "origin/main"],
    [CURRENT, { here: true, onGitHub: false }, "this clone"],
    [CURRENT, { here: false, onGitHub: true }, "GitHub"],
  ])("refuses %j with %j", (tree, existing, words) => {
    expect(tagRefusal(tree, "v0.2.4", existing)).toContain(words);
  });
});
