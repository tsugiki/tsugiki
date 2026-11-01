import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import fs from "node:fs";
const host = process.env.TAURI_DEV_HOST;

// PROTOTYPE: stands in for the Rust command that reads a file at a commit from gh-rev's own mirror
// (ADR 0002), for expanding context in the diff. GET /__proto/file?repo=owner/name&pr=N&oid=SHA&path=P.
// A PR head that isn't in the mirror yet is fetched from GitHub into it (read-only on GitHub; the
// user's clone is only borrowed from through alternates, never written).
/** @returns {import("vite").Plugin} */
function protoFiles() {
  return {
    name: "proto-files",
    async configureServer(server) {
      const root = server.config.root.replace(/\\/g, "/");
      const mirrors = `${root}/.proto-mirrors`;
      // The snapshot script's mirror helper (plain JS, untyped: loaded by URL so the type check skips it).
      const gitHelper = `file:///${root.replace(/^\//, "")}/scripts/fetch/git.mjs`;
      const { openMirror } = await import(/* @vite-ignore */ gitHelper);
      server.middlewares.use("/__proto/file", (req, res) => {
        const u = new URL(/** @type {any} */ (req).url ?? "", "http://localhost");
        const [repo, pr, oid, path] = ["repo", "pr", "oid", "path"].map((k) => u.searchParams.get(k) ?? "");
        /** @param {number} code @param {string} why */
        const fail = (code, why) => { res.statusCode = code; res.end(why); };
        if (!/^[\w.-]+\/[\w.-]+$/.test(repo) || !/^\d+$/.test(pr) || !/^[0-9a-f]{40}$/.test(oid) || !path || path.startsWith("-")) return fail(400, "bad request");
        const dir = `${mirrors}/${repo.replace("/", "__")}`;
        if (!fs.existsSync(`${dir}/HEAD`)) return fail(404, "no mirror for this Repository: run proto:snapshot");
        const m = openMirror(dir, repo);
        if (!m.has(oid)) { m.fetch([], [+pr]); if (!m.has(oid)) m.fetchCommit(oid); }
        if (!m.has(oid)) return fail(404, "the PR's head commit can't be fetched");
        const text = m.show(oid, path);
        if (text == null) return fail(404, "no such file at the PR's head (deleted?) or too large");
        res.setHeader("content-type", "text/plain; charset=utf-8");
        res.end(text);
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [sveltekit(), protoFiles()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      // 4. and the throwaway git mirrors the snapshot script keeps
      ignored: ["**/src-tauri/**", "**/.proto-mirrors/**"],
    },
  },
}));
