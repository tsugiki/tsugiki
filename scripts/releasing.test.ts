import { describe, expect, it } from "vitest";

import { notesFor } from "./release-notes.ts";
import {
  type Tree,
  byVersion,
  isoDate,
  isReleaseBranch,
  mainCi,
  prepareRefusal,
  releaseBranch,
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

describe("byVersion", () => {
  it("puts the earliest version first, a short one as if padded, and other titles last", () => {
    const titles = ["Backlog", "1.0.0", "0.10.0", "0.4.1", "0.2.0", "0.4", "Someday"];
    expect(titles.sort(byVersion)).toEqual([
      "0.2.0",
      "0.4",
      "0.4.1",
      "0.10.0",
      "1.0.0",
      "Backlog",
      "Someday",
    ]);
  });
});

describe("releaseBranch", () => {
  it("names the branch after the version, as a release branch", () => {
    expect(releaseBranch("0.2.4")).toBe("release/0.2.4");
    expect(isReleaseBranch(releaseBranch("0.2.4"))).toBe(true);
  });

  it("tells another branch from a release branch", () => {
    expect(isReleaseBranch("860-fold-scripts")).toBe(false);
  });
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

  it("writes a heading that the release check reads back", () => {
    expect(notesFor(withReleasedWip(CHANGELOG, "0.2.4", "2026-11-20"), "0.2.4")).toContain("### Added");
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

  it("fails when the version has a section with a date the release check can't read", () => {
    const changelog = `${CHANGELOG}\n## Version 0.2.4 (unreleased)\n`;
    expect(() => withReleasedWip(changelog, "0.2.4", "2026-11-20")).toThrow("already");
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
    expect(tagRefusal(CURRENT, "v0.2.4", nowhere, "passed")).toBeUndefined();
  });

  it.each([
    [{ ...CURRENT, branch: "release/0.2.4" }, nowhere, "main"],
    [{ ...CURRENT, dirty: true }, nowhere, "Commit"],
    [{ ...CURRENT, head: "def" }, nowhere, "origin/main"],
    [CURRENT, { here: true, onGitHub: false }, "this clone"],
    [CURRENT, { here: false, onGitHub: true }, "GitHub"],
  ])("refuses %j with %j", (tree, existing, words) => {
    expect(tagRefusal(tree, "v0.2.4", existing, "passed")).toContain(words);
  });

  it.each([
    ["running", "still running"],
    ["failed", "failed"],
    ["missing", "hasn't run"],
  ] as const)("refuses a main whose CI is %s", (ci, words) => {
    expect(tagRefusal(CURRENT, "v0.2.4", nowhere, ci)).toContain(words);
  });
});

describe("mainCi", () => {
  it.each([
    ['[{"status":"completed","conclusion":"success"}]', "passed"],
    [
      '[{"status":"completed","conclusion":"failure"},{"status":"completed","conclusion":"success"}]',
      "failed",
    ],
    ['[{"status":"completed","conclusion":"cancelled"}]', "failed"],
    ['[{"status":"in_progress","conclusion":""}]', "running"],
    ["[]", "missing"],
    ["{}", "missing"],
    ["not json", "missing"],
  ] as const)("reads %s as %s", (listed, expected) => {
    expect(mainCi(listed)).toBe(expected);
  });
});
