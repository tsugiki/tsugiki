import { expect, it } from "vitest";

const components = import.meta.glob<string>("/src/**/*.svelte", {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Each line of `source` that picks a font family: `font-family`, or a `font` shorthand that does more than
 * inherit. */
const fontFamilies = (source: string) =>
  source
    .split("\n")
    .filter((line) => /\bfont-family\s*:/.test(line) || /\bfont\s*:(?!\s*inherit\s*;)/.test(line));

it("finds a component's own font family", () => {
  expect(
    fontFamilies("  font-family: serif;\n  font: 12px monospace;\n  font: inherit;\n  font-size: 1px;"),
  ).toEqual(["  font-family: serif;", "  font: 12px monospace;"]);
});

// window.css gives the fonts (#598), so the window keeps to two of them.
it("leaves the font family to the window's stylesheet", () => {
  const found = Object.entries(components).flatMap(([path, source]) =>
    fontFamilies(source).map((line) => `${path}: ${line.trim()}`),
  );

  expect(Object.keys(components).length).toBeGreaterThan(10);
  expect(found).toEqual([]);
});
