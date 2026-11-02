// @vitest-environment node
import { join, resolve } from "node:path";
import { RuleTester } from "eslint";
import svelteParser from "svelte-eslint-parser";
import ts from "typescript-eslint";
import { describe, it } from "vitest";
import layers from "./layers.ts";

RuleTester.describe = describe;
RuleTester.it = it;

const SRC = resolve(import.meta.dirname, "../src");
const at = (path: string) => join(SRC, ...path.split("/"));
const options = [{ src: SRC }];

const tester = new RuleTester({ languageOptions: { parser: ts.parser } });
const svelteTester = new RuleTester({
  languageOptions: { parser: svelteParser, parserOptions: { parser: ts.parser } },
});

tester.run("layers/imports", layers.rules.imports, {
  valid: [
    {
      filename: at("lib/ui/Row.svelte.ts"),
      options,
      code: `
        import { untrack } from "svelte";
        import hljs from "highlight.js/lib/core";
        import { workbench } from "$lib/state/workbench.svelte";
        import { rows } from "../core/rows";
        import { openUrl } from "$lib/platform/open";
        import { Cell } from "./Cell";
      `,
    },
    {
      filename: at("lib/data/issues.ts"),
      options,
      code: `import { invoke } from "@tauri-apps/api/core"; import type { Row } from "$lib/core/rows";`,
    },
    {
      filename: at("lib/state/about.svelte.ts"),
      options,
      code: `import { commands } from "$lib/data/bindings"; let about = $state(); export { about };`,
    },
    {
      filename: at("routes/+page.ts"),
      options,
      code: `import { goto } from "$app/navigation"; import type { PageLoad } from "./$types";`,
    },
    {
      filename: at("lib/core/rows.ts"),
      options,
      code: `import { range } from "./range"; // no window here\nexport const rows = range(3);`,
    },
    {
      filename: at("lib/ui/Lazy.ts"),
      options,
      code: `const a = await import(name); const b = await import(\`$lib/ui/\${name}\`); const c = await import(1);`,
    },
    {
      filename: at("lib/core/rows.ts"),
      options,
      code: `export const n = undeclaredButNotBanned();`,
    },
    {
      filename: at("app.d.ts"),
      options,
      code: `import type { Anything } from "anything";`,
    },
    {
      filename: at("lib/state/about.svelte.test.ts"),
      options: [{ src: SRC, extraPackages: ["vitest"] }],
      code: `import { expect } from "vitest"; import { about } from "./about.svelte";`,
    },
  ],
  invalid: [
    {
      filename: at("lib/ui/About.ts"),
      options,
      code: `import type { About } from "$lib/data/bindings";`,
      errors: [{ message: "ui must not import data ($lib/data/bindings)" }],
    },
    {
      filename: at("lib/ui/About.ts"),
      options,
      code: `import { commands } from "../data/bindings";`,
      errors: [{ message: "ui must not import data (../data/bindings)" }],
    },
    {
      filename: at("lib/ui/About.ts"),
      options,
      code: `const bindings = await import("$lib/data/bindings"); const again = await import(\`$lib/data/bindings\`); export * from "$lib/data/bindings"; export { commands } from "../data/bindings";`,
      errors: [
        { message: "ui must not import data ($lib/data/bindings)" },
        { message: "ui must not import data ($lib/data/bindings)" },
        { message: "ui must not import data ($lib/data/bindings)" },
        { message: "ui must not import data (../data/bindings)" },
      ],
    },
    {
      filename: at("routes/+page.ts"),
      options,
      code: `import { commands } from "$lib/data/bindings";`,
      errors: [{ message: "routes must not import data ($lib/data/bindings)" }],
    },
    {
      filename: at("lib/state/about.svelte.ts"),
      options,
      code: `import { invoke } from "@tauri-apps/api/core";`,
      errors: [{ message: "state must not import package @tauri-apps/api/core" }],
    },
    {
      filename: at("lib/core/rows.ts"),
      options,
      code: `import { writable } from "svelte/store";`,
      errors: [{ message: "core must not import package svelte/store" }],
    },
    {
      filename: at("lib/core/rows.ts"),
      options,
      code: `export const width = () => window.innerWidth + document.body.clientWidth;\nlet n = $state(0);\nconst title = globalThis.document.title + navigator.language;`,
      errors: [
        { message: "core must not use window" },
        { message: "core must not use document" },
        { message: "core must not use $state" },
        { message: "core must not use globalThis" },
        { message: "core must not use navigator" },
      ],
    },
    {
      filename: at("lib/constructor/x.ts"),
      options,
      code: `export const x = 1;`,
      errors: [{ message: "not in a known layer (core, data, state, platform, ui, routes)" }],
    },
    {
      // The real config declares the browser's globals, and they resolve differently from undeclared names.
      filename: at("lib/core/rows.ts"),
      options,
      languageOptions: { globals: { document: "readonly", console: "readonly" } },
      code: `const fetch = () => 1; console.log(document.title, fetch());`,
      errors: [{ message: "core must not use document" }],
    },
    {
      filename: at("lib"),
      options,
      code: `export const x = 1;`,
      errors: [{ message: "not in a known layer (core, data, state, platform, ui, routes)" }],
    },
    {
      filename: at("lib/ui/About.ts"),
      options,
      code: `import "$lib/";`,
      errors: [{ message: "ui must not import  ($lib/)" }],
    },
    {
      filename: at("lib/helpers.ts"),
      options,
      code: `export const x = 1;`,
      errors: [{ message: "not in a known layer (core, data, state, platform, ui, routes)" }],
    },
  ],
});

svelteTester.run("layers/imports in components", layers.rules.imports, {
  valid: [
    {
      filename: at("lib/ui/AboutLine.svelte"),
      options,
      code: `<script lang="ts">import type { About } from "$lib/state/about.svelte"; let { about }: { about: About } = $props();</script><p>{about.name}</p>`,
    },
  ],
  invalid: [
    {
      filename: at("lib/ui/AboutLine.svelte"),
      options,
      code: `<script lang="ts">import { commands } from "$lib/data/bindings";</script>`,
      errors: [{ message: "ui must not import data ($lib/data/bindings)" }],
    },
  ],
});
