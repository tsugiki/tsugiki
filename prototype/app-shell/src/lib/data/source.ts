// PROTOTYPE — throwaway. Data source: the only module that knows where the data comes from. Here it is
// the per-Repository snapshots that scripts/snapshot.mjs wrote; in the real app this becomes Tauri
// `invoke` calls to the Rust backend, which does the fetching and the git work.
import type { Snapshot } from "$lib/core/types";

const files = import.meta.glob<Snapshot>("./fixtures/*.json", { eager: true, import: "default" });

/** The added Repositories, CPython first, then by name. */
export const snapshots: Snapshot[] = Object.values(files).sort((a, b) =>
  +(b.repo.name === "python/cpython") - +(a.repo.name === "python/cpython") || a.repo.name.localeCompare(b.repo.name));
