import { expect, it } from "vitest";
import { wordAt } from "./pick";

it("finds the identifier around an offset, at its start, inside it and at its end", () => {
  const text = "    return self.max_len + 1";
  expect(wordAt(text, 11)).toEqual({ start: 11, end: 15 });
  expect(wordAt(text, 13)).toEqual({ start: 11, end: 15 });
  expect(wordAt(text, 23)).toEqual({ start: 16, end: 23 });
});

it("finds no word in the spaces and punctuation between words", () => {
  const text = "a  +  b";
  expect(wordAt(text, 3)).toBeUndefined();
  expect(wordAt(text, 4)).toBeUndefined();
  expect(wordAt("", 0)).toBeUndefined();
  expect(wordAt("abc", -1)).toBeUndefined();
});

it("counts letters beyond ASCII, digits and underscores as a word's, in UTF-16 code units", () => {
  expect(wordAt("x = __init__2", 7)).toEqual({ start: 4, end: 13 });
  expect(wordAt("print(größe)", 8)).toEqual({ start: 6, end: 11 });
  expect(wordAt("𝑥𝑦 = 1", 1)).toEqual({ start: 0, end: 4 });
});
