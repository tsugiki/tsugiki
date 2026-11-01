// PROTOTYPE — throwaway. Data source: the only module that knows where the data comes from. Here it is
// the per-Repository snapshots that scripts/snapshot.mjs wrote; in the real app this becomes Tauri
// `invoke` calls to the Rust backend, which does the fetching and the git work.
import type { Snapshot } from "$lib/core/types";

const files = import.meta.glob<Snapshot>("./fixtures/*.json", { eager: true, import: "default" });

/** The added Repositories, CPython first, then by name. */
export const snapshots: Snapshot[] = Object.values(files).sort((a, b) =>
  +(b.repo.name === "python/cpython") - +(a.repo.name === "python/cpython") || a.repo.name.localeCompare(b.repo.name));

/** A file at a commit, from gh-rev's mirror; null when there's none (a deleted file, no mirror). In the
 *  real app: a Rust command. Here: the dev server (vite.config.js) reads the prototype's mirror. */
export async function fileAt(repo: string, pr: number, oid: string, path: string): Promise<string | null> {
  try {
    const r = await fetch(`/__proto/file?${new URLSearchParams({ repo, pr: String(pr), oid, path })}`);
    return r.ok ? await r.text() : null;
  } catch { return null; }
}
