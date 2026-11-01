// PROTOTYPE — throwaway. Files at a commit, loaded on demand for expanding context in the diff, and
// kept for the session. A SvelteMap so the line arrays themselves aren't made reactive.
import { SvelteMap } from "svelte/reactivity";
import { fileAt } from "$lib/data/source";

export type Source = { state: "loading" } | { state: "ok"; lines: string[] } | { state: "missing" };
const sources = new SvelteMap<string, Source>();

export const sourceOf = (oid: string, path: string): Source | undefined => sources.get(`${oid}:${path}`);

export function loadSource(repo: string, pr: number, oid: string, path: string) {
  const k = `${oid}:${path}`;
  if (sources.has(k)) return;
  sources.set(k, { state: "loading" });
  fileAt(repo, pr, oid, path).then((t) => {
    if (t == null) return sources.set(k, { state: "missing" });
    const lines = t.split("\n");
    if (lines.at(-1) === "") lines.pop();
    sources.set(k, { state: "ok", lines });
  });
}
