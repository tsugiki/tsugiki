// SvelteKit can't load a TypeScript config, so this one stays JavaScript with JSDoc types (ADR 0007).
// Tauri has no server: adapter-static with an index.html fallback makes a single-page app.
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import("@sveltejs/kit").Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({ fallback: "index.html" }),
  },
};

export default config;
