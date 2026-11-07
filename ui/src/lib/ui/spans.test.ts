import { expect, it } from "vitest";
import { pieces } from "./spans";

it("cuts a row's text at its spans, and leaves the text between them plain", () => {
  expect(
    pieces("x5 = 'five'", [
      { start: 3, end: 4, highlight: "keyword" },
      { start: 5, end: 11, highlight: "string" },
    ]),
  ).toEqual([
    { text: "x5 ", highlight: null },
    { text: "=", highlight: "keyword" },
    { text: " ", highlight: null },
    { text: "'five'", highlight: "string" },
  ]);
});

it("keeps a row with no spans whole, and an empty row empty", () => {
  expect(pieces("line 17", [])).toEqual([{ text: "line 17", highlight: null }]);
  expect(pieces("", [])).toEqual([]);
});

it("counts in UTF-16 code units, as the core does", () => {
  expect(pieces('"😀" # ü', [{ start: 0, end: 4, highlight: "string" }])).toEqual([
    { text: '"😀"', highlight: "string" },
    { text: " # ü", highlight: null },
  ]);
});
