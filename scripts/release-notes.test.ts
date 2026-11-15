import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { notesFor, notesOfWorkspace, releasePage, versionOfTag } from "./release-notes.ts";

const CARGO_TOML = `[workspace]
members = ["crates/core"]

[workspace.package]
edition = "2024"
version = "0.2.0"

[workspace.dependencies]
serde = { version = "1.0.0" }
`;

const CHANGELOG = `# Changelog

## WIP

### Added

- Something not released yet.

## Version 0.2.0 (2026-12-01)

### Added

- The second thing.

### Fixed

- A bug.

## Version 0.1.0 (2026-11-20)

### Added

- The first thing.
`;

describe("versionOfTag", () => {
  it("gives the version when the tag names the workspace's version", () => {
    expect(versionOfTag("v0.2.0", CARGO_TOML)).toBe("0.2.0");
  });

  it("fails when the tag names another version, because the downloads would carry Cargo.toml's", () => {
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

  it("fails when the changelog has no section for the version, and says what the heading looks like", () => {
    expect(() => notesFor(CHANGELOG, "0.3.0")).toThrow(
      'CHANGELOG.md has no section "## Version 0.3.0 (YYYY-MM-DD)".',
    );
  });

  it("doesn't take a section whose heading only starts with the version", () => {
    expect(() => notesFor("## Version 0.1.01 (2026-11-20)\n\n- Later.\n", "0.1.0")).toThrow("no section");
  });

  it("doesn't take a heading without a date", () => {
    expect(() => notesFor("## Version 0.1.0\n\n- First.\n", "0.1.0")).toThrow("no section");
  });

  it("fails when the version's section is empty", () => {
    expect(() =>
      notesFor("## Version 0.2.0 (2026-12-01)\n\n## Version 0.1.0 (2026-11-20)\n\n- First.\n", "0.2.0"),
    ).toThrow("CHANGELOG.md's section for 0.2.0 is empty.");
  });

  it("fails when two sections name the version, whatever their dates", () => {
    expect(() =>
      notesFor(
        "## Version 0.1.0 (2026-12-01)\n\n- Second.\n\n## Version 0.1.0 (2026-11-20)\n\n- First.\n",
        "0.1.0",
      ),
    ).toThrow("CHANGELOG.md has 2 sections for 0.1.0.");
  });
});

describe("notesOfWorkspace", () => {
  it("gives the section of the version Cargo.toml says", () => {
    expect(notesOfWorkspace(CARGO_TOML, CHANGELOG)).toBe(
      "### Added\n\n- The second thing.\n\n### Fixed\n\n- A bug.",
    );
  });

  it("fails when a bumped version has no section yet", () => {
    expect(() => notesOfWorkspace(CARGO_TOML, "## WIP\n\n- Something.\n")).toThrow(
      'CHANGELOG.md has no section "## Version 0.2.0 (YYYY-MM-DD)".',
    );
  });

  it("fails when Cargo.toml doesn't say the workspace's version", () => {
    expect(() => notesOfWorkspace("[workspace]\n", CHANGELOG)).toThrow(
      "Cargo.toml has no version in [workspace.package].",
    );
  });

  describe("before the first release, at version 0.0.0", () => {
    const UNRELEASED = '[workspace.package]\nversion = "0.0.0"\n';

    it("gives the WIP section", () => {
      expect(notesOfWorkspace(UNRELEASED, CHANGELOG)).toBe("### Added\n\n- Something not released yet.");
    });

    it("fails without a WIP section", () => {
      expect(() => notesOfWorkspace(UNRELEASED, "# Changelog\n")).toThrow(
        'CHANGELOG.md has no section "## WIP".',
      );
    });
  });
});

describe("releasePage", () => {
  const page = releasePage("### Added\n\n- The first thing.");

  /** The lines of `text` that run `gh attestation verify`. */
  const verifyCommands = (text: string): string[] =>
    text.split(/\r?\n/).filter((line) => line.startsWith("gh attestation verify "));

  it("starts with the notes, then says which file is for which system", () => {
    expect(page).toMatch(/^### Added\n\n- The first thing\.\n\n### Download\n/);
    expect(page).toContain("- Windows: the `.zip`");
    expect(page).toContain("- Apple silicon Macs: the `.dmg`");
  });

  it("checks a download with the README's command, so the two can't drift apart", () => {
    const readme = readFileSync("../README.md", "utf8");
    expect(verifyCommands(page)).toHaveLength(1);
    expect(verifyCommands(page)).toEqual(verifyCommands(readme));
  });

  describe("joins the lines the changelog wraps, since GitHub breaks a release's lines where its body does", () => {
    /** The notes as the release page shows them, without the footer. */
    const notesOn = (notes: string): string => releasePage(notes).split("\n\n### Download\n")[0] ?? "";

    it("joins a wrapped paragraph into one line", () => {
      expect(notesOn("The first half of a sentence\nand its second half.\n\nA second paragraph.")).toBe(
        "The first half of a sentence and its second half.\n\nA second paragraph.",
      );
    });

    it("joins a list item's indented lines into the item", () => {
      expect(
        notesOn("- A click on a word picks it, and a\n  double-click selects it.\n- Another item."),
      ).toBe("- A click on a word picks it, and a double-click selects it.\n- Another item.");
    });

    it("keeps a heading on its own line, whether a paragraph comes before it or after it", () => {
      expect(notesOn("A paragraph\nwrapped.\n### Added\nA list follows.")).toBe(
        "A paragraph wrapped.\n### Added\nA list follows.",
      );
    });

    it("joins a line that starts with a ticket's number, which isn't a heading", () => {
      expect(notesOn("- The fix that\n  #749 asked for.")).toBe("- The fix that #749 asked for.");
      expect(notesOn("A paragraph that\n#749 asked for.")).toBe("A paragraph that #749 asked for.");
    });

    it("keeps a nested list item on its own line", () => {
      expect(notesOn("- An item\n  - A nested one")).toBe("- An item\n  - A nested one");
    });

    it("keeps the lines inside a fence as they are, and starts a paragraph after it", () => {
      expect(notesOn("Run this:\n\n```\nfirst line\n  second line\n```\nThen\nthis.")).toBe(
        "Run this:\n\n```\nfirst line\n  second line\n```\nThen this.",
      );
    });
  });
});
