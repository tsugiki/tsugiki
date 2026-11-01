// PROTOTYPE — throwaway. Data source: the only module that knows where the data comes from. Here it is
// the snapshot that scripts/snapshot.mjs wrote; in the real app this becomes Tauri `invoke` calls to
// the Rust backend, which does the fetching and the git work.
import type { Snapshot } from "$lib/core/types";
import fixture from "./fixture.json";

export const snapshot = fixture as unknown as Snapshot;
