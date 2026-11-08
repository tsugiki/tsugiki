import { expect, it } from "vitest";
import { pieces } from "./spans";

it("cuts a row's text at its spans, and leaves the text between them plain", () => {
  expect(
    pieces("x5 = 'five'", [
      { start: 3, end: 4, highlight: "keyword" },
      { start: 5, end: 11, highlight: "string" },
    ]),
  ).toEqual([
    { text: "x5 ", highlight: null, found: null },
    { text: "=", highlight: "keyword", found: null },
    { text: " ", highlight: null, found: null },
    { text: "'five'", highlight: "string", found: null },
  ]);
});

it("keeps a row with no spans whole, and an empty row empty", () => {
  expect(pieces("line 17", [])).toEqual([{ text: "line 17", highlight: null, found: null }]);
  expect(pieces("", [])).toEqual([]);
});

it("counts in UTF-16 code units, as the core does", () => {
  expect(pieces('"😀" # ü', [{ start: 0, end: 4, highlight: "string" }])).toEqual([
    { text: '"😀"', highlight: "string", found: null },
    { text: " # ü", highlight: null, found: null },
  ]);
});

it("cuts a row's text at the search's matches too, marking the one the tab is on", () => {
  expect(
    pieces(
      "x = 'xyz'",
      [{ start: 4, end: 9, highlight: "string" }],
      [
        { start: 0, end: 1, current: false },
        { start: 2, end: 6, current: true },
      ],
    ),
  ).toEqual([
    { text: "x", highlight: null, found: "match" },
    { text: " ", highlight: null, found: null },
    { text: "= ", highlight: null, found: "current" },
    { text: "'x", highlight: "string", found: "current" },
    { text: "yz'", highlight: "string", found: null },
  ]);
});
