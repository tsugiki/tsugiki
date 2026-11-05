import { describe, expect, it } from "vitest";

import { addedLines, findings, RULES } from "./prose.ts";

function rulesFor(words: string): string[] {
  return findings("a.md", `${words}\n`).map((found) => found.rule);
}

describe("findings", () => {
  it.each([
    ["adverb", "The sync reads it properly."],
    ["filler", "A click just asks again."],
    ["em-dash", "The sync stops — and waits."],
    ["wh-opener", "What a sync does."],
    ["wh-opener", "The sync stops. When it starts, it asks."],
    ["lazy-extreme", "Each refresh always asks."],
    ["throat-clearing", "Here's what the sync does."],
    ["throat-clearing", "It's worth noting that the sync stops."],
    ["contrast", "The sync isn't slow, it's careful."],
    ["contrast", "Not the sync, but the store."],
    ["mention", "Filled by @someone today."],
    ["mention", "@someone filled it."],
  ])("flags %s in %j", (rule, words) => {
    expect(rulesFor(words)).toContain(rule);
  });

  it.each([
    "The sync reads the reply and applies it.",
    "An early family of rules can rely on this only.",
    "A step reads the supply list in July.",
    "Somewhat fine, because the store knows when and why.",
  ])("passes %j", (words) => {
    expect(rulesFor(words)).toEqual([]);
  });

  it("reads past inline code, since code isn't prose", () => {
    expect(rulesFor("The `just check` recipe calls `@types/node` and `fly`.")).toEqual([]);
  });

  it("says where each match is, the rule and the line's words", () => {
    expect(findings("a.rs", "fn a() {}\n/// It reads properly.\n")).toEqual([
      { at: 2, rule: "adverb", text: "It reads properly." },
    ]);
  });

  it("lists every rule a line breaks", () => {
    expect(rulesFor("It just reads properly.")).toEqual(["adverb", "filler"]);
  });

  it("reads only the lines it's told to", () => {
    const source = "It reads properly.\nIt reads badly.\n";
    expect(findings("a.md", source, new Set([2])).map((found) => found.at)).toEqual([2]);
  });

  it("gives each rule a name and a reason", () => {
    const names = RULES.map((rule) => rule.name);
    expect(new Set(names).size).toBe(names.length);
    expect(RULES.every((rule) => rule.reason.length > 0)).toBe(true);
  });
});

describe("addedLines", () => {
  it("reads a diff's added lines for each file", () => {
    const diff = [
      "diff --git a/a.md b/a.md",
      "--- a/a.md",
      "+++ b/a.md",
      "@@ -1,2 +1,3 @@",
      "+new",
      "@@ -9 +10,2 @@",
      "+new",
      "+new",
      "diff --git a/c.md b/c.md",
      "new file mode 100644",
      "--- /dev/null",
      "+++ b/c.md",
      "@@ -0,0 +1 @@",
      "+new",
      "",
    ].join("\n");

    expect(addedLines(diff)).toEqual(
      new Map([
        ["a.md", new Set([1, 2, 3, 10, 11])],
        ["c.md", new Set([1])],
      ]),
    );
  });

  it("leaves a file that only lost lines", () => {
    const diff = ["--- a/a.md", "+++ b/a.md", "@@ -1,2 +0,0 @@", "-old", "-old", ""].join("\n");
    expect(addedLines(diff)).toEqual(new Map());
  });

  it("leaves a deleted file", () => {
    const diff = ["--- a/a.md", "+++ /dev/null", "@@ -1 +0,0 @@", "-old", ""].join("\n");
    expect(addedLines(diff)).toEqual(new Map());
  });
});
