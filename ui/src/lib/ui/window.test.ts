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
 * first `>` outside quotes and `{…}`, so an arrow function in a handler doesn't end it. */
const scrimTags = (source: string) => {
  const markup = source.replace(/<(script|style)\b[\s\S]*?<\/\1>/g, "");
  return [...markup.matchAll(/<[a-zA-Z]/g)]
    .map(({ index }) => {
      let depth = 0;
      let quote = "";
      for (let i = index + 1; i < markup.length; i++) {
        const c = markup[i];
        if (quote) quote = c === quote ? "" : quote;
        else if (c === '"' || c === "'" || c === "`") quote = c;
        else if (c === "{") depth++;
        else if (c === "}") depth--;
        else if (c === ">" && depth === 0) return markup.slice(index, i + 1);
      }
      return markup.slice(index);
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
      '<script>if (a<b) x();</script>\n<div class="scrim wide" data-testid="a-scrim" onmousedown={() => close()}>\n<div data-testid="b">',
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

it("puts each overlay on the scrim, and the performance HUD above it", () => {
  const scrimRule = /^\.scrim \{([^}]*)\}/m.exec(windowCss)?.[1] ?? "";
  const tags = Object.values(sources).flatMap(scrimTags);

  expect(zIndices(scrimRule)).toEqual(["var(--layer-overlay)"]);
  expect(tags.length).toBeGreaterThanOrEqual(4);
  expect(tags.filter((tag) => !hasScrimClass(tag))).toEqual([]);
  expect(zIndices(sources["src/lib/ui/PerformanceHud.svelte"] ?? "")).toEqual(["var(--layer-hud)"]);
});
