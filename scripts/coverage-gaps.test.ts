import { describe, expect, it } from "vitest";

import { filesWithGaps, unrunRegions } from "./coverage-gaps.ts";

const LCOV = [
  "SF:X:\\repo\\crates\\core\\src\\whole.rs",
  "FNF:2",
  "FNH:2",
  "LF:10",
  "LH:10",
  "end_of_record",
  "SF:X:\\repo\\crates\\core\\src\\line.rs",
  "FNF:2",
  "FNH:2",
  "LF:10",
  "LH:9",
  "end_of_record",
  "SF:X:\\repo\\crates\\core\\src\\function.rs",
  "FNF:3",
  "FNH:2",
  "LF:10",
  "LH:10",
  "end_of_record",
  "",
].join("\r\n");

const REPORT = [
  "X:\\repo\\crates\\core\\src\\line.rs:",
  "    1|       |// A comment.",
  "    2|      9|    let a = |b| c;",
  "                              ^0",
  "    3|      0|    never();",
  "    4|      9|    let d = e(f)?;",
  "                              ^0",
  "   14|     1k|    d();",
  "",
  "X:\\repo\\crates\\core\\src\\other.rs:",
  "    5|      0|    never();",
  "",
].join("\n");

describe("filesWithGaps", () => {
  it("names each file short of its lines or functions, with the counts", () => {
    expect(filesWithGaps(LCOV)).toEqual([
      { path: "X:\\repo\\crates\\core\\src\\line.rs", lines: "9/10", functions: "2/2" },
      { path: "X:\\repo\\crates\\core\\src\\function.rs", lines: "10/10", functions: "2/3" },
    ]);
  });

  it("finds nothing in a whole report", () => {
    expect(filesWithGaps("SF:a.rs\nFNF:1\nFNH:1\nLF:1\nLH:1\nend_of_record\n")).toEqual([]);
  });
});

describe("unrunRegions", () => {
  it("gives the lines that never ran and the regions on lines that did, but no `?`, for one file only", () => {
    expect(unrunRegions(REPORT, "X:\\repo\\crates\\core\\src\\line.rs")).toEqual([
      { at: 2, source: "let a = |b| c;", whole: false },
      { at: 3, source: "never();", whole: true },
    ]);
  });

  it("finds nothing for a file the report doesn't hold", () => {
    expect(unrunRegions(REPORT, "X:\\repo\\missing.rs")).toEqual([]);
  });

  it("reads Windows line endings", () => {
    const report = "a.rs:\r\n    2|      0|    never();\r\n";
    expect(unrunRegions(report, "a.rs")).toEqual([{ at: 2, source: "never();", whole: true }]);
  });

  it("skips a marker with no line above it", () => {
    expect(unrunRegions("a.rs:\n    ^0\n", "a.rs")).toEqual([]);
  });
});
