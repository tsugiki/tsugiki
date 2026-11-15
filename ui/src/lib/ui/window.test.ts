import { readdirSync, readFileSync } from "node:fs";
import { expect, it } from "vitest";

const WINDOW_CSS = "src/lib/ui/window.css";

/** Every component, stylesheet and script in src but the tests, by path. They come from disk: an `?raw`
 * import hands a test each stylesheet as "". */
const sources: Record<string, string> = Object.fromEntries(
  readdirSync("src", { recursive: true, encoding: "utf8" })
    .map((path) => `src/${path.split(/[\\/]/).join("/")}`)
    .filter((path) => /\.(svelte|css|ts)$/.test(path) && !/\.(test|bench)\.ts$/.test(path))
    .map((path) => [path, readFileSync(path, "utf8")]),
);

const windowCss = sources[WINDOW_CSS] ?? "";

/** Each line of `source` that picks a font family: `font-family`, Svelte's `style:font-family`, or a
 * `font` shorthand that does more than inherit. */
const fontFamilies = (source: string) =>
  source
    .split("\n")
    .filter((line) => /\bfont-family\s*[:=]/.test(line) || /\bfont\s*[:=](?!\s*inherit\s*;)/.test(line));

it("finds a component's own font family", () => {
  expect(
    fontFamilies(
      '  font-family: serif;\n  font: 12px monospace;\n  font: inherit;\n  font-size: 1px;\n<p style:font-family="serif">',
    ),
  ).toEqual(["  font-family: serif;", "  font: 12px monospace;", '<p style:font-family="serif">']);
});

// window.css gives the fonts (#598), so the window keeps to two of them.
it("leaves the font family to the window's stylesheet", () => {
  const found = Object.entries(sources)
    .filter(([path]) => /\.(svelte|css)$/.test(path) && path !== WINDOW_CSS)
    .flatMap(([path, source]) => fontFamilies(source).map((line) => `${path}: ${line.trim()}`));

  expect(Object.keys(sources).length).toBeGreaterThan(10);
  expect(windowCss).toContain(":root");
  expect(found).toEqual([]);
});

/** Each z-index that `source` sets, as written: in a rule, a Svelte `style:z-index` or a script's
 * `zIndex`. */
const zIndices = (source: string) =>
  [...source.matchAll(/\b(?:z-index|zIndex)\s*[:=]\s*(?:"([^"]*)"|'([^']*)'|\{([^}]*)\}|([^;,}\n]*))/g)].map(
    (match) => (match[1] ?? match[2] ?? match[3] ?? match[4] ?? "").trim(),
  );

/** The stacking layers window.css names on the root, as `--layer-overlay: 10;`. */
const layers = new Map(
  [...windowCss.matchAll(/^\s*--layer-([\w-]+):\s*(\d+);/gm)].map((match) => [match[1], Number(match[2])]),
);

/** Whether a z-index is a whole number below the lowest layer, or names a layer. */
const fits = (value: string) => {
  const layer = /^var\(--layer-([\w-]+)\)$/.exec(value)?.[1];
  return layer ? layers.has(layer) : /^\d+$/.test(value) && Number(value) < Math.min(...layers.values());
};

/** The opening tag of each element in `source`'s markup whose test id ends in `-scrim`. A tag runs to the
 * first `>` outside quotes and `{…}`, so an arrow function in a handler doesn't end it. A `<` inside the
 * `<script>` or `<style>` block starts no tag. */
const scrimTags = (source: string) => {
  const blocks = [...source.matchAll(/<(script|style)\b[\s\S]*?<\/\1>/g)].map(({ index, 0: block }) => [
    index,
    index + block.length,
  ]);
  return [...source.matchAll(/<[a-zA-Z]/g)]
    .filter(({ index }) => !blocks.some(([start = 0, end = 0]) => index >= start && index < end))
    .map(({ index }) => {
      let depth = 0;
      let quote = "";
      for (let i = index + 1; i < source.length; i++) {
        const c = source[i];
        if (quote) quote = c === quote ? "" : quote;
        else if (c === '"' || c === "'" || c === "`") quote = c;
        else if (c === "{") depth++;
        else if (c === "}") depth--;
        else if (c === ">" && depth === 0) return source.slice(index, i + 1);
      }
      return source.slice(index);
    })
    .filter((tag) => /\bdata-testid="[\w-]+-scrim"/.test(tag));
};

/** Whether `tag` carries the `scrim` class among its classes. */
const hasScrimClass = (tag: string) =>
  /\bclass="([^"]*)"/.exec(tag)?.[1]?.split(/\s+/).includes("scrim") ?? false;

it("reads each z-index as written, and finds each scrim's tag", () => {
  expect(
    zIndices(
      '  z-index: 2;\n<p style:z-index="30" style:z-index={20}>\n  z-index: var(--layer-hud);\nel.style.zIndex = String(z);\n{ zIndex: 5, top: 0 }',
    ),
  ).toEqual(["2", "30", "20", "var(--layer-hud)", "String(z)", "5"]);
  expect(
    scrimTags(
      '<script>const t = `<p data-testid="t-scrim">`;</script>\n<style>a<b[data-testid="s-scrim"] {}</style>\n<div class="scrim wide" data-testid="a-scrim" onmousedown={() => close()}>\n<div data-testid="b">',
    ),
  ).toEqual(['<div class="scrim wide" data-testid="a-scrim" onmousedown={() => close()}>']);
  expect(hasScrimClass('<div class="scrim wide">')).toBe(true);
  expect(hasScrimClass('<div class="scrimmed">')).toBe(false);
});

// A sticky bar or an icon in a pane painted over the palette's text box (#703). The palette and the dialogs
// sit on window.css's scrim, on the overlay layer, and nothing else in the window reaches it.
it("keeps every z-index under the overlay layer, apart from the layers themselves", () => {
  const found = Object.entries(sources).flatMap(([path, source]) =>
    zIndices(source)
      .filter((value) => !fits(value))
      .map((value) => `${path}: ${value}`),
  );

  expect(layers.get("overlay")).toBeGreaterThan(0);
  expect(layers.get("hud")).toBeGreaterThan(layers.get("overlay") ?? Infinity);
  expect(found).toEqual([]);
});

/** Each block in `css` as the selectors before it, split at their commas, and the text inside it, in the
 * order the blocks close. A rule inside an at-rule, such as a keyframe or a rule under `@media`, reads as a
 * rule of its own, and the at-rule's text leaves it out. Comments count for nothing. */
const rulesOf = (css: string) => {
  const rules: { selectors: string[]; body: string }[] = [];
  /** The selectors before each block still open, innermost last. */
  const open: string[] = [];
  let text = "";
  for (let at = 0; at < css.length; at++) {
    const c = css.charAt(at);
    if (css.startsWith("/*", at)) {
      const end = css.indexOf("*/", at + 2);
      at = end < 0 ? css.length : end + 1;
    } else if (c === "{") {
      open.push(text);
      text = "";
    } else if (c === "}") {
      const selectors = (open.pop() ?? "").split(",").map((selector) => selector.trim());
      rules.push({ selectors, body: text });
      text = "";
    } else {
      text += c;
    }
  }
  return rules;
};

/** The bodies of the rules in `css` with a selector that `wanted` takes. */
const bodiesOf = (css: string, wanted: (selector: string) => boolean) =>
  rulesOf(css)
    .filter((rule) => rule.selectors.some(wanted))
    .map((rule) => rule.body);

it("reads a stylesheet's rules, nested ones and those with a comment among their selectors too", () => {
  const css =
    "/* A note. */\n.a,\n.b.c {\n  top: 0;\n}\n\n@media (x) {\n  .d /* over */ {\n    left: 0;\n  }\n}\n";

  expect(rulesOf(css)).toEqual([
    { selectors: [".a", ".b.c"], body: "\n  top: 0;\n" },
    { selectors: [".d"], body: "\n    left: 0;\n  " },
    { selectors: ["@media (x)"], body: "\n" },
  ]);
  expect(bodiesOf(css, (selector) => selector.startsWith(".b"))).toEqual(["\n  top: 0;\n"]);
  expect(rulesOf("/* Never closed")).toEqual([]);
});

it("puts each overlay on the scrim, and the performance HUD above it", () => {
  const scrimRule = bodiesOf(windowCss, (selector) => selector === ".scrim").join("");
  const tags = Object.values(sources).flatMap(scrimTags);

  expect(zIndices(scrimRule)).toEqual(["var(--layer-overlay)"]);
  expect(tags.length).toBeGreaterThanOrEqual(4);
  expect(tags.filter((tag) => !hasScrimClass(tag))).toEqual([]);
  expect(zIndices(sources["src/lib/ui/PerformanceHud.svelte"] ?? "")).toEqual(["var(--layer-hud)"]);
});

// The file view and the Definition peek drew their code at the window's 14px, and marked the line Go to
// definition landed on in another colour than the diff's (#754). window.css now gives both, once, and the
// component tests check that the code and its landed line take the classes.
it("draws code at one size, and the line Go to definition lands on in one colour, from window.css", () => {
  const diff = sources["src/lib/ui/DiffRows.svelte"] ?? "";
  const peek = sources["src/lib/ui/PeekCode.svelte"] ?? "";
  const styleOf = (source: string) => /<style>([\s\S]*)<\/style>/.exec(source)?.[1] ?? "";
  const marks = bodiesOf(styleOf(diff), (selector) => /^\.mark\b/.test(selector));

  expect(bodiesOf(windowCss, (selector) => selector === ".code-lines")).toEqual([
    expect.stringContaining("font-size: 12px;"),
  ]);
  expect(bodiesOf(windowCss, (selector) => selector === ".code-lines .landed")).toEqual([
    expect.stringContaining("var(--find-current)"),
  ]);
  expect([styleOf(diff), styleOf(peek)].filter((style) => style.includes("font-size"))).toEqual([]);
  expect(marks.length).toBeGreaterThan(0);
  expect(marks.filter((body) => body.includes("background"))).toEqual([]);
});
