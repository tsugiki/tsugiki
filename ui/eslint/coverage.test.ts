// @vitest-environment node
import { RuleTester } from "eslint";
import svelteParser from "svelte-eslint-parser";
import ts from "typescript-eslint";
import { describe, it } from "vitest";
import coverage from "./coverage.ts";

RuleTester.describe = describe;
RuleTester.it = it;

const message = "a coverage exclusion needs a `// coverage: <reason>` comment on the line above";
const missing = (code: string) => ({ code, errors: [{ message }] });

new RuleTester({ languageOptions: { parser: ts.parser } }).run("coverage/reason", coverage.rules.reason, {
  valid: [
    `const a = 1; // an ordinary comment`,
    `// coverage: the webview never takes this path\n/* v8 ignore next */\nconst a = 1;`,
    `function f() {\n  // coverage: the webview never takes this path\n  /* v8 ignore next */\n  return 1;\n}`,
    `/* v8 ignore stop */\nconst a = 1;`,
  ],
  invalid: [
    missing(`/* v8 ignore next */\nconst a = 1;`),
    missing(`/* v8 ignore start */\nconst a = 1;`),
    missing(`/* v8 ignore file */\nconst a = 1;`),
    missing(`/* v8 ignore if */\nconst a = 1;`),
    missing(`/* v8 ignore else */\nconst a = 1;`),
    missing(`/* istanbul ignore next */\nconst a = 1;`),
    missing(`/* c8 ignore next */\nconst a = 1;`),
    missing(`/* node:coverage ignore next */\nconst a = 1;`),
    missing(`/** v8 ignore next */\nconst a = 1;`),
    missing(`/* @preserve v8 ignore start */\nconst a = 1;`),
    missing(`// v8 ignore next\nconst a = 1;`),
    missing(`// another comment\n/* v8 ignore next */\nconst a = 1;`),
    missing(`// coverage:\n/* v8 ignore next */\nconst a = 1;`),
    missing(`// coverage: too far away\n\n/* v8 ignore next */\nconst a = 1;`),
    missing(`foo(); // coverage: about foo\n/* v8 ignore next */\nconst a = 1;`),
    missing(`/* coverage: a block comment */\n/* v8 ignore next */\nconst a = 1;`),
  ],
});

new RuleTester({
  languageOptions: { parser: svelteParser, parserOptions: { parser: ts.parser } },
}).run("coverage/reason in components", coverage.rules.reason, {
  valid: [
    `<!-- an ordinary comment -->\n<p>a</p>`,
    `<!-- coverage: only macOS shows this -->\n<!-- v8 ignore start -->\n<p>a</p>\n<!-- v8 ignore stop -->`,
  ],
  invalid: [
    missing(`<script lang="ts">/* v8 ignore next */\nconst a = 1;</script>`),
    missing(`<!-- v8 ignore start -->\n<p>a</p>\n<!-- v8 ignore stop -->`),
    missing(`<!-- coverage: -->\n<!-- v8 ignore start -->\n<p>a</p>`),
  ],
});
