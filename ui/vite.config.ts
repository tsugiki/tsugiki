import { sveltekit } from "@sveltejs/kit/vite";
import { svelteTesting } from "@testing-library/svelte/vite";
import { defineConfig } from "vitest/config";

// Tauri's devUrl expects this port, and a busy port must fail rather than move.
export default defineConfig({
  plugins: [sveltekit(), svelteTesting()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "eslint/**/*.test.ts", "../scripts/**/*.test.ts"],
    // TypeScript's coverage gate (ADR 0009). `npm run coverage -w ui` turns it on.
    coverage: {
      provider: "v8",
      // In the ignored .temp beside the other test output; CI hands lcov.info to Codecov from there.
      reportsDirectory: "../.temp/vitest-coverage",
      allowExternal: true,
      include: ["src/**/*.{ts,svelte}", "eslint/**/*.ts", "../scripts/**/*.ts"],
      exclude: [
        // coverage: generated from the Rust commands, and every test mocks it.
        "src/lib/data/bindings.ts",
        // coverage: benchmarks run under CodSpeed, not as tests.
        "src/**/*.bench.ts",
        // coverage: a command line around cargo, with its logic in the files it calls.
        "../scripts/bindings.ts",
        "../scripts/rust-coverage.ts",
        "../scripts/release-check.ts",
        // coverage: reads the real manifest and exits, with its logic in token-crate.ts.
        "../scripts/token-deps.ts",
        // coverage: command lines around git, with their logic in the files they call.
        "../scripts/doc-width.ts",
        "../scripts/prose-check.ts",
        "../scripts/changes.ts",
        // coverage: runs the real Rust gates or skips them, with its logic in scope.ts.
        "../scripts/rust-gates.ts",
        // coverage: asks GitHub about a real PR and deletes a real branch, which no unit test can do.
        "../scripts/merged.ts",
        // coverage: runs `just check`, pushes and opens a real PR, with its logic in pr.ts.
        "../scripts/ship.ts",
        // coverage: lists a real milestone's tickets on GitHub, with its logic in pr.ts and tickets.ts.
        "../scripts/next.ts",
        // coverage: reads Claude Code's call from stdin, with its logic in shell-edits.ts.
        "../scripts/shell-edits-hook.ts",
        // coverage: checks the real window with real keys, which no unit test can do.
        "../scripts/smoke.ts",
        // coverage: zips the real build and starts it, which no unit test can do.
        "../scripts/portable-zip.ts",
      ],
      thresholds: { 100: true },
      // lcovonly is for Codecov, with paths from the repo root like the Rust report's.
      reporter: ["text", ["lcovonly", { projectRoot: ".." }]],
    },
  },
});
