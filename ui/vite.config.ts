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
      ],
      thresholds: { 100: true },
      // lcovonly is for Codecov, with paths from the repo root like the Rust report's.
      reporter: ["text", ["lcovonly", { projectRoot: ".." }]],
    },
  },
});
