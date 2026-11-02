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
    include: ["src/**/*.test.ts", "eslint/**/*.test.ts"],
  },
});
