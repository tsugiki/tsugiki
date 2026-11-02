// ESLint for the scripts at the repo root. ui/ has its own config, with the layer rule.
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import { defineConfig } from "eslint/config";
import globals from "globals";
import ts from "typescript-eslint";
import coverage from "./ui/eslint/coverage.ts";

export default defineConfig(js.configs.recommended, ts.configs.recommended, prettier, {
  languageOptions: { globals: globals.node },
  plugins: { coverage },
  rules: { "coverage/reason": "error" },
});
