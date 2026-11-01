// The UI's layers, and what each may import (carried over from the app-shell prototype's check):
//
//   core      plain TypeScript: only core; no Svelte, no DOM
//   data      where data comes from: core, and Tauri's IPC for the generated bindings
//   state     the user's state, reactive: core, data, svelte
//   platform  what the desktop shell does (open a URL): Tauri only
//   ui        components and presentation: state, core, platform, svelte, highlight.js; never data
//   routes    the entry point: ui, state (to pick the start-up Repository) and SvelteKit
//
// A layer is a folder of src/lib, or src/routes. Files elsewhere in src are not checked.
import { dirname, join, relative, resolve, sep } from "node:path";
import type { ESLint, Rule } from "eslint";
import type * as ESTree from "estree";

const ALLOWED: Record<string, { layers: string[]; packages: string[] }> = {
  core: { layers: ["core"], packages: [] },
  data: { layers: ["data", "core"], packages: ["@tauri-apps/api"] },
  state: { layers: ["state", "core", "data"], packages: ["svelte"] },
  platform: { layers: ["platform"], packages: ["@tauri-apps"] },
  ui: { layers: ["ui", "state", "core", "platform"], packages: ["svelte", "highlight.js"] },
  routes: { layers: ["routes", "ui", "state"], packages: ["svelte", "$app"] },
};
// Core must run anywhere (a worker, tests, the Rust port's reference): no browser globals or runes.
const CORE_BANNED = new Set([
  "document",
  "window",
  "globalThis",
  "self",
  "navigator",
  "location",
  "DOMParser",
  "localStorage",
  "sessionStorage",
  "indexedDB",
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "requestAnimationFrame",
  "$state",
  "$derived",
  "$effect",
  "$props",
]);

type Options = { src: string; extraPackages?: string[] };

const layerOf = (src: string, file: string) => {
  const parts = relative(src, file).split(sep);
  return parts[0] === "lib" ? parts[1] : parts[0];
};

const imports: Rule.RuleModule = {
  meta: {
    type: "problem",
    docs: { description: "Keep each UI layer to the layers and packages it may import" },
    schema: [
      {
        type: "object",
        properties: {
          src: { type: "string" },
          extraPackages: { type: "array", items: { type: "string" } },
        },
        required: ["src"],
        additionalProperties: false,
      },
    ],
    messages: {
      layer: "{{from}} must not import {{to}} ({{spec}})",
      package: "{{from}} must not import package {{spec}}",
      global: "core must not use {{name}}",
      unknown: "not in a known layer ({{layers}})",
    },
  },
  create(context) {
    const { src, extraPackages = [] } = context.options[0] as Options;
    const file = context.filename;
    const top = relative(src, file).split(sep)[0];
    if (top !== "lib" && top !== "routes") return {};
    const from = layerOf(src, file) ?? "";
    const rule = Object.hasOwn(ALLOWED, from) ? ALLOWED[from] : undefined;
    if (!rule) {
      return {
        Program(node) {
          context.report({ node, messageId: "unknown", data: { layers: Object.keys(ALLOWED).join(", ") } });
        },
      };
    }
    const packages = [...rule.packages, ...extraPackages];

    const check = (source: ESTree.Expression | null | undefined) => {
      if (!source) return;
      // A dynamic import of a template without placeholders is still a fixed specifier.
      const spec =
        source.type === "Literal"
          ? source.value
          : source.type === "TemplateLiteral" && source.expressions.length === 0
            ? source.quasis[0]?.value.cooked
            : undefined;
      if (typeof spec !== "string") return;
      const node = source as Rule.Node;
      if (spec.startsWith(".") || spec.startsWith("$lib/")) {
        const target = spec.startsWith("$lib/")
          ? join(src, "lib", spec.slice(5))
          : resolve(dirname(file), spec);
        const to = layerOf(src, target) ?? "";
        if (!rule.layers.includes(to)) context.report({ node, messageId: "layer", data: { from, to, spec } });
      } else if (!packages.some((p) => spec === p || spec.startsWith(`${p}/`))) {
        context.report({ node, messageId: "package", data: { from, spec } });
      }
    };

    return {
      ImportDeclaration: (node) => check(node.source),
      ImportExpression: (node) => check(node.source),
      ExportAllDeclaration: (node) => check(node.source),
      ExportNamedDeclaration: (node) => check(node.source),
      "Program:exit"(node) {
        if (from !== "core") return;
        const scope = context.sourceCode.getScope(node);
        const implicit = scope.variables.filter((v) => v.defs.length === 0 && CORE_BANNED.has(v.name));
        const references = [...scope.through, ...implicit.flatMap((v) => v.references)];
        for (const { identifier } of references) {
          if (CORE_BANNED.has(identifier.name)) {
            context.report({ node: identifier, messageId: "global", data: { name: identifier.name } });
          }
        }
      },
    };
  },
};

const layers = { meta: { name: "tsugiki-layers" }, rules: { imports } } satisfies ESLint.Plugin;
export default layers;
