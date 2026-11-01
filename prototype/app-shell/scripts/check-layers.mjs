// PROTOTYPE — throwaway. Fails if a layer imports from one it shouldn't:
//
//   core   plain TypeScript: only core; no Svelte, no DOM
//   data   where data comes from: core (types) and its own files
//   state  the user's state, reactive: core, data, svelte
//   platform  what the desktop shell does (open a URL): Tauri plugins only
//   ui     components and presentation: state, core, platform, svelte, highlight.js; never data
//   routes the entry point: ui, state (to pick the start-up Repository) and SvelteKit
//
//   npm run proto:check
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SRC = resolve(dirname(fileURLToPath(import.meta.url)), "../src");
const ALLOWED = {
  core: { layers: ["core"], packages: [] },
  data: { layers: ["data", "core"], packages: [] },
  state: { layers: ["state", "core", "data"], packages: ["svelte"] },
  platform: { layers: ["platform"], packages: ["@tauri-apps"] },
  ui: { layers: ["ui", "state", "core", "platform"], packages: ["svelte", "highlight.js"] },
  routes: { layers: ["routes", "ui", "state"], packages: ["svelte", "$app"] },
};
// Core must run anywhere (a worker, tests, the Rust port's reference): no browser globals or runes.
const CORE_BANNED = /\b(document|window|DOMParser|localStorage|fetch)\b|\$(state|derived|effect|props)\b/;

const layerOf = (abs) => {
  const parts = relative(SRC, abs).split(sep);
  return parts[0] === "lib" ? parts[1] : parts[0];
};
const files = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
  e.isDirectory() ? files(join(dir, e.name)) : /\.(ts|svelte)$/.test(e.name) ? [join(dir, e.name)] : []);

const problems = [];
for (const file of [...files(join(SRC, "lib")), ...files(join(SRC, "routes"))]) {
  const layer = layerOf(file);
  const rule = ALLOWED[layer];
  const where = relative(SRC, file);
  if (!rule) { problems.push(`${where}: not in a known layer (${Object.keys(ALLOWED).join(", ")})`); continue; }
  const text = readFileSync(file, "utf8");
  const specs = [...text.matchAll(/(?:^|[\s;])import\s+(?:type\s+)?(?:[^'";]*?\sfrom\s+)?["']([^"']+)["']|import\(\s*["']([^"']+)["']\s*\)/g)].map((m) => m[1] ?? m[2]);
  for (const spec of specs) {
    if (spec.startsWith(".") || spec.startsWith("$lib/")) {
      const target = spec.startsWith("$lib/") ? join(SRC, "lib", spec.slice(5)) : resolve(dirname(file), spec);
      const to = layerOf(target);
      if (!rule.layers.includes(to)) problems.push(`${where}: ${layer} must not import ${to} (${spec})`);
    } else if (!rule.packages.some((p) => spec === p || spec.startsWith(p + "/"))) problems.push(`${where}: ${layer} must not import package ${spec}`);
  }
  if (layer === "core") {
    const code = text.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    const m = CORE_BANNED.exec(code);
    if (m) problems.push(`${where}: core must not use ${m[0]}`);
  }
}
if (problems.length) { console.error(problems.join("\n")); process.exit(1); }
console.log("layers ok");
