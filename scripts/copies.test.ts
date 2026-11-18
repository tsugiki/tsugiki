import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ALLOWED, type Allowed, type Copy, describeCopy, find, judge, readReport, SCANS } from "./copies.ts";

/** The folders `tree` made, which each test deletes after it. */
const trees: string[] = [];

afterEach(() => {
  for (const root of trees.splice(0)) rmSync(root, { recursive: true, force: true });
});

/** A folder in the system's temp folder holding `files`, each path relative to it with its text. */
function tree(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "tsugiki-copies-"));
  trees.push(root);
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
  return root;
}

/** The scan named `name`. */
const scan = (name: string) => {
  const found = SCANS.find((each) => each.name === name);
  if (found === undefined) throw new Error(`no scan named ${name}`);
  return found;
};

/**
 * The copies the scan named `name` finds in a folder holding `files` and each folder it reads, as
 * `describeCopy` words them.
 */
const copiesIn = (name: string, files: Record<string, string>) => {
  const root = tree(files);
  for (const path of scan(name).paths) mkdirSync(join(root, path), { recursive: true });
  return find(scan(name), root, join(root, ".temp", "jscpd")).map(describeCopy);
};

// A test helper of six lines, as RepositoryHome.test.ts held one of FilesTab.test.ts's before #856.
const PRESSED = `
const pressed = () => {
  const group = within(screen.getByRole("group", { name: "Files" }));
  return ["All files", "One file"].map((name) =>
    group.getByRole("button", { name }).getAttribute("aria-pressed"),
  );
};
`;

// A Rust test helper of five lines, as the store's tests held one per module before #858.
const STORE = `
fn store() -> (TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(&folder.path().join("store.db"), &MIGRATIONS).unwrap();
    (folder, store)
}
`;

describe("the UI scan", () => {
  it("finds a five-line helper that one test file copies from another", () => {
    const copies = copiesIn("UI and scripts", {
      "ui/src/lib/ui/A.test.ts": `import { screen, within } from "@testing-library/svelte";\n${PRESSED}`,
      "ui/src/lib/ui/B.test.ts": `const answer = 42;\n${PRESSED}\nexport { answer };\n`,
    });
    expect(copies).toEqual([
      expect.stringMatching(/^ui\/src\/lib\/ui\/A\.test\.ts:\d+-\d+ and ui\/src\/lib\/ui\/B/),
    ]);
  });

  it("reads Svelte, the e2e specs and the scripts", () => {
    const copies = copiesIn("UI and scripts", {
      "ui/src/lib/ui/Pane.svelte": `<script lang="ts">\n${PRESSED}</script>\n`,
      "ui/e2e/pane.spec.ts": PRESSED,
      "scripts/pane.ts": PRESSED,
    });
    // jscpd pairs each later copy with the first it read, so three copies make two pairs.
    expect(copies).toHaveLength(2);
    for (const file of ["ui/src/lib/ui/Pane.svelte", "ui/e2e/pane.spec.ts", "scripts/pane.ts"]) {
      expect(copies.join("\n")).toContain(`${file}:`);
    }
  });

  it("leaves out the generated bindings and key map", () => {
    expect(
      copiesIn("UI and scripts", {
        "ui/src/lib/data/bindings.ts": PRESSED,
        "ui/src/lib/data/keymap.ts": PRESSED,
        "ui/src/lib/ui/A.test.ts": PRESSED,
      }),
    ).toEqual([]);
  });
});

describe("the Rust scan", () => {
  it("finds a five-line helper that one test module copies from another", () => {
    const copies = copiesIn("Rust", {
      "crates/core/src/store/issues/tests.rs": `use super::*;\n${STORE}`,
      "crates/core/src/store/filters/tests.rs": `use crate::store::*;\n${STORE}\nconst N: u8 = 3;\n`,
    });
    expect(copies).toHaveLength(1);
  });

  it("leaves out the highlighter's samples", () => {
    expect(
      copiesIn("Rust", {
        "crates/core/src/highlight/samples/rust.rs": STORE,
        "crates/core/src/store/tests.rs": STORE,
      }),
    ).toEqual([]);
  });
});

const copy = (first: string, second: string): Copy => ({
  tokens: 60,
  first: { file: first, start: 1, end: 7 },
  second: { file: second, start: 10, end: 16 },
});

describe("judge", () => {
  const allowed: Allowed = {
    files: ["b.ts", "a.ts"],
    tokens: [60],
    why: "They change for different reasons.",
  };

  it("passes a repeat inside one file", () => {
    expect(judge([copy("a.ts", "a.ts")], [])).toEqual({ copies: [], unused: [] });
  });

  it("passes a copy between two files the list allows with its size, in either order", () => {
    expect(judge([copy("a.ts", "b.ts")], [allowed])).toEqual({ copies: [], unused: [] });
    expect(judge([copy("b.ts", "a.ts")], [allowed])).toEqual({ copies: [], unused: [] });
  });

  it("fails a copy between two other files, and names an allowed size it no longer finds", () => {
    expect(judge([copy("a.ts", "c.ts")], [allowed])).toEqual({
      copies: [copy("a.ts", "c.ts")],
      unused: [{ files: ["b.ts", "a.ts"], tokens: 60 }],
    });
  });

  it("fails a copy of another size between two files the list allows", () => {
    const grown = { ...copy("a.ts", "b.ts"), tokens: 75 };
    expect(judge([grown], [allowed])).toEqual({
      copies: [grown],
      unused: [{ files: ["b.ts", "a.ts"], tokens: 60 }],
    });
  });
});

describe("readReport", () => {
  it("names each place relative to the root, with forward slashes, from Windows' long paths too", () => {
    const place = (name: string, start: number, end: number) => ({
      name,
      start,
      end,
      startLoc: {},
      endLoc: {},
    });
    const report = {
      statistics: {},
      duplicates: [
        {
          format: "rust",
          tokens: 72,
          lines: 6,
          firstFile: place(join("X:", "root", "crates", "a.rs"), 3, 8),
          secondFile: place(`\\\\?\\${join("X:", "root", "crates", "b.rs")}`, 10, 15),
          fragment: "fn a() {}",
        },
      ],
    };
    expect(readReport(JSON.stringify(report), join("X:", "root"))).toEqual([
      {
        tokens: 72,
        first: { file: "crates/a.rs", start: 3, end: 8 },
        second: { file: "crates/b.rs", start: 10, end: 15 },
      },
    ]);
  });

  it("refuses a report without its list of duplicates", () => {
    expect(() => readReport("{}", ".")).toThrow("jscpd's report has no list of duplicates");
  });
});

describe("describeCopy", () => {
  it("names both places and the size", () => {
    expect(describeCopy(copy("a.ts", "b.ts"))).toBe("a.ts:1-7 and b.ts:10-16 hold the same 60 tokens");
  });
});

describe("ALLOWED", () => {
  it("says why each repeat stays, and names two files that exist and a size for each copy", () => {
    for (const { files, tokens, why } of ALLOWED) {
      expect(why.trim(), files.join(" and ")).not.toBe("");
      expect(tokens.length, why).toBeGreaterThan(0);
      expect(files[0], why).not.toBe(files[1]);
      for (const file of files) expect(existsSync(join("..", file)), file).toBe(true);
    }
  });
});
