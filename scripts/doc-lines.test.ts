import { describe, expect, it } from "vitest";

import { docLines, isDocFile, wideLines } from "./doc-lines.ts";

const WIDE = "word ".repeat(30).trim();

function texts(path: string, source: string): string[] {
  return docLines(path, source).map((line) => line.text);
}

describe("isDocFile", () => {
  it("takes Markdown and the files we write comments in", () => {
    const paths = ["CONTEXT.md", "a/b.rs", "a/b.ts", "a/B.svelte", "ci.yml", "Cargo.toml", "justfile"];
    expect(paths.filter(isDocFile)).toEqual(paths);
  });

  it("leaves data, lock files and generated code alone", () => {
    const paths = ["a.json", "a.png", "ui/src/lib/data/bindings.ts", "Cargo.lock", "package-lock.json"];
    expect(paths.filter(isDocFile)).toEqual([]);
  });
});

describe("docLines", () => {
  it("reads all of a Markdown file but its fences and tables, with line numbers from 1", () => {
    const source = "# Title\n\nSome words.\n\n```sh\nnot prose\n```\n\n| a | b |\n\nMore words.\n";
    expect(docLines("a.md", source).map(({ at, text }) => [at, text])).toEqual([
      [1, "# Title"],
      [3, "Some words."],
      [11, "More words."],
    ]);
  });

  it("treats a file of no known kind as prose, so a draft of a PR body reads", () => {
    expect(texts("draft.txt", "Some words.\n")).toEqual(["Some words."]);
  });

  it("reads Rust's comments and docstrings and leaves its code", () => {
    const source = '//! The module.\nuse a;\n/// A thing.\nfn a() {\n    // Why.\n    *b = "// c";\n}\n';
    expect(texts("a.rs", source)).toEqual(["The module.", "A thing.", "Why."]);
  });

  it("reads a block comment to its end", () => {
    const source = "/** One.\n * Two.\n */\nconst a = 1;\n/* Three. */\nconst b = 2;\n";
    expect(texts("a.ts", source)).toEqual(["One.", "Two.", "Three."]);
  });

  it("reads Svelte's markup comments", () => {
    const source = "<div>\n  <!-- One. -->\n  <!-- Two.\n  Three. -->\n</div>\n";
    expect(texts("a.svelte", source)).toEqual(["One.", "Two.", "Three."]);
  });

  it("reads a # comment in files that use them, and not in Rust", () => {
    expect(texts("ci.yml", "# One.\na: 1\n  # Two.\n")).toEqual(["One.", "Two."]);
    expect(texts("justfile", "# One.\ncheck: lint\n")).toEqual(["One."]);
    expect(texts("a.rs", "#[test]\n")).toEqual([]);
  });

  it("skips a fence or a table inside a docstring", () => {
    const source = "/// Words.\n/// ```\n/// code\n/// ```\n/// | a |\n";
    expect(texts("a.rs", source)).toEqual(["Words."]);
  });

  it("keeps the whole line for measuring", () => {
    expect(docLines("a.rs", "    // Why.\n")[0]?.raw).toBe("    // Why.");
  });
});

describe("wideLines", () => {
  it("names a Markdown line, a comment and a docstring past 110 columns", () => {
    expect(wideLines("a.md", `${WIDE}\n`)).toEqual([{ at: 1, width: WIDE.length }]);
    expect(wideLines("a.rs", `// ${WIDE}\n`)).toEqual([{ at: 1, width: WIDE.length + 3 }]);
    expect(wideLines("a.rs", `fn a() {}\n/// ${WIDE}\n`)).toEqual([{ at: 2, width: WIDE.length + 4 }]);
  });

  it("counts a character once however many bytes it takes", () => {
    const fits = "é ".repeat(55);
    expect(wideLines("a.md", `${fits}\n`)).toEqual([]);
    expect(wideLines("a.md", `${fits}é\n`)).toEqual([{ at: 1, width: 111 }]);
  });

  it("passes exactly 110 columns", () => {
    expect(wideLines("a.md", `${"a ".repeat(54)}bb\n`)).toEqual([]);
  });

  it("passes what can't wrap: tables, fences, headings and a line with no space to break at", () => {
    expect(wideLines("a.md", `| ${WIDE} |\n`)).toEqual([]);
    expect(wideLines("a.md", `\`\`\`\n${WIDE}\n\`\`\`\n`)).toEqual([]);
    expect(wideLines("a.md", `# ${WIDE}\n`)).toEqual([]);
    const url = `https://example.com/${"a".repeat(120)}`;
    expect(wideLines("a.md", `${url}\n`)).toEqual([]);
    expect(wideLines("a.md", `- ${url}\n`)).toEqual([]);
    expect(wideLines("a.rs", `// ${url}\n`)).toEqual([]);
  });

  it("leaves wide code to the formatters", () => {
    expect(wideLines("a.rs", `const A: &str = "${WIDE}";\n`)).toEqual([]);
  });
});
