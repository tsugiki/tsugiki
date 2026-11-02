import { resolve } from "node:path";
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import svelte from "eslint-plugin-svelte";
import { defineConfig, globalIgnores } from "eslint/config";
import globals from "globals";
import ts from "typescript-eslint";
import layers from "./eslint/layers.ts";
import svelteConfig from "./svelte.config.js";

const src = resolve(import.meta.dirname, "src");

export default defineConfig(
  globalIgnores([".svelte-kit/", "build/", "test-results/", "src/lib/data/bindings.ts"]),
  js.configs.recommended,
  ts.configs.recommended,
  svelte.configs.recommended,
  prettier,
  svelte.configs.prettier,
  {
    languageOptions: { globals: globals.browser },
  },
  {
    // Tooling runs in Node; the webview code under src/ doesn't.
    files: ["*.config.ts", "*.config.js", "eslint/**/*.ts", "e2e/**/*.ts"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        extraFileExtensions: [".svelte"],
        parser: ts.parser,
        svelteConfig,
      },
    },
  },
  {
    files: ["src/**/*.ts", "src/**/*.svelte"],
    plugins: { layers },
    rules: { "layers/imports": ["error", { src }] },
  },
  {
    files: ["src/**/*.test.ts"],
    rules: { "layers/imports": ["error", { src, extraPackages: ["vitest", "@testing-library/svelte"] }] },
  },
);
