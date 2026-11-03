// @vitest-environment node
import { describe, expect, it } from "vitest";
import { checkReports, relativeLcov } from "./lcov.ts";

const report = (file: string, body: string[]) => [`SF:${file}`, ...body, "end_of_record", ""].join("\n");
const check = (...texts: string[]) => checkReports(texts.map((text, i) => ({ path: `os${i}.lcov`, text })));

describe("relativeLcov", () => {
  it("makes Windows and macOS paths the same", () => {
    const windows = relativeLcov(report("D:\\a\\tsugiki\\crates\\core\\src\\lib.rs", []), "D:\\a\\tsugiki");
    const macos = relativeLcov(
      report("/Users/runner/tsugiki/crates/core/src/lib.rs", []),
      "/Users/runner/tsugiki",
    );
    expect(windows).toBe(report("crates/core/src/lib.rs", []));
    expect(macos).toBe(windows);
  });

  it("leaves a path outside the root as it is", () => {
    expect(relativeLcov(report("/elsewhere/lib.rs", []), "/root")).toBe(report("/elsewhere/lib.rs", []));
  });

  it("doesn't take a sibling folder for the root", () => {
    expect(relativeLcov(report("/root-2/lib.rs", []), "/root")).toBe(report("/root-2/lib.rs", []));
  });
});

describe("one report", () => {
  it("passes with everything reached", () => {
    expect(check(report("a.rs", ["FN:1,_Rf", "FNDA:1,_Rf", "DA:1,1"]))).toEqual([]);
  });

  it("names the lines and functions no test reached", () => {
    const text = report("a.rs", ["FN:7,_Rf", "FNDA:0,_Rf", "DA:8,0", "DA:7,0", "DA:9,1"]);
    expect(check(text)).toEqual(["a.rs: lines 7, 8; functions at 7"]);
  });

  it("names only what is missing", () => {
    expect(check(report("a.rs", ["DA:1,0"]))).toEqual(["a.rs: lines 1"]);
    expect(check(report("a.rs", ["FN:1,_Rf", "FNDA:0,_Rf"]))).toEqual(["a.rs: functions at 1"]);
  });

  it("reads several files, and CRLF line endings", () => {
    const text = (report("a.rs", ["DA:1,0"]) + report("b.rs", ["DA:2,0"])).replaceAll("\n", "\r\n");
    expect(check(text)).toEqual(["a.rs: lines 1", "b.rs: lines 2"]);
  });

  it("reads the function form that gives an end line", () => {
    expect(check(report("a.rs", ["FN:7,9,_Rf", "FNDA:0,_Rf"]))).toEqual(["a.rs: functions at 7"]);
  });

  it("reads function names with commas", () => {
    expect(check(report("a.rs", ["FN:7,f<a,b>", "FNDA:0,f<a,b>"]))).toEqual(["a.rs: functions at 7"]);
    expect(check(report("a.rs", ["FN:7,f<a,b>", "FNDA:1,f<a,b>"]))).toEqual([]);
  });

  it("counts a function with no hits recorded as not reached", () => {
    expect(check(report("a.rs", ["FN:7,_Rf"]))).toEqual(["a.rs: functions at 7"]);
  });

  it("counts a function built twice, in the test build and as a dependency, as one", () => {
    const dep = "_RNvCsdtlCI5pCLax_12tsugiki_core5about";
    const test = "_RNvCsh6L4n5iw8cx_12tsugiki_core5about";
    const text = report("a.rs", [`FN:13,${dep}`, `FN:13,${test}`, `FNDA:0,${dep}`, `FNDA:1,${test}`]);
    expect(check(text)).toEqual([]);
  });

  it("counts a generic function as one when its two builds' crate hashes differ in length", () => {
    // From a real report: the shorter hash moves what `Result`'s path points back at, from `BY_` to `BX_`.
    const dep = "_RINvMNtCsa66rCdMECK7_12tsugiki_core5storeNtB3_5Store4callINtNtBY_6result6ResultEEB5_";
    const test = "_RINvMNtCssScWlLHOsg_12tsugiki_core5storeNtB3_5Store4callINtNtBX_6result6ResultEEB5_";
    const text = report("a.rs", [`FN:76,${dep}`, `FN:76,${test}`, `FNDA:0,${dep}`, `FNDA:29,${test}`]);
    expect(check(text)).toEqual([]);
  });

  it("doesn't let a closure hide another that starts on the same line", () => {
    const text = report("a.rs", ["FN:5,_RNCa0", "FN:5,_RNCa1", "FNDA:3,_RNCa0", "FNDA:0,_RNCa1"]);
    expect(check(text)).toEqual(["a.rs: functions at 5"]);
  });

  it("names a line once when two closures on it were missed", () => {
    const text = report("a.rs", ["FN:5,_RNCa0", "FN:5,_RNCa1", "FNDA:0,_RNCa0", "FNDA:0,_RNCa1"]);
    expect(check(text)).toEqual(["a.rs: functions at 5"]);
  });

  it("refuses hits for a function the report never declared", () => {
    expect(() => check(report("a.rs", ["FNDA:1,_Rf"]))).toThrow("a.rs: hits for an unknown function, _Rf");
  });
});

describe("reports from two OSes", () => {
  const windows = report("a.rs", ["FN:1,_RNvCsaaa_1a1f", "FNDA:1,_RNvCsaaa_1a1f", "DA:1,1", "DA:2,0"]);
  const macos = report("a.rs", ["FN:1,__RNvCsbbb_1a1f", "FNDA:0,__RNvCsbbb_1a1f", "DA:1,0", "DA:2,3"]);

  it("counts code covered on either OS as covered", () => {
    expect(check(windows, macos)).toEqual([]);
  });

  it("still fails on what neither OS reached", () => {
    expect(check(report("a.rs", ["DA:4,0"]), report("a.rs", ["DA:4,0"]))).toEqual(["a.rs: lines 4"]);
  });

  it("holds a file built on only one OS to the same bar", () => {
    expect(check(windows, macos, report("mac.rs", ["DA:5,0"]))).toEqual(["mac.rs: lines 5"]);
  });

  it("refuses a report with no files, even if the other is complete", () => {
    expect(check(report("a.rs", ["DA:1,1"]), "")).toEqual(["os1.lcov: no coverage data"]);
  });
});
