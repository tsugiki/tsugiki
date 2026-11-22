// A stand-in for the Files tab that the rail's tree reads (#604), whose diff and file on screen a test
// changes and the tree follows, as runes make it reactive.
import type { TreeSource } from "../state/file-tree.svelte";
import type { PrFiles } from "../state/pr-files.svelte";

/** What the tree reads of the Files tab, every field of it open to a test, the file on screen too. */
export type FakeTreeSource = { -readonly [K in keyof TreeSource]: TreeSource[K] };

/** A Files tab with the diff `view`, the core's answer, and no file on screen yet. */
export function treeSource(view: PrFiles | null | undefined): FakeTreeSource {
  const files = $state<FakeTreeSource>({
    view,
    error: undefined,
    unfetched: undefined,
    waitsForNetwork: false,
    onScreen: undefined,
  });
  return files;
}
