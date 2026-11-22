// A stand-in for the Files tab that the rail's tree reads (#604), whose diff and file on screen a test
// changes and the tree follows, as runes make it reactive.
import type { TreeSource } from "../state/file-tree.svelte";
import type { PrFiles } from "../state/pr-files.svelte";

/** What the tree reads of the Files tab: the diff and the file on screen, which a test sets, and a count of
 * the changes of the file on screen, by its path, that rises as the Files tab's does. */
export class FakeTreeSource implements TreeSource {
  readonly error: TreeSource["error"] = undefined;
  readonly unfetched: TreeSource["unfetched"] = undefined;
  readonly waitsForNetwork = false;
  #view = $state<PrFiles | null | undefined>();
  #onScreen = $state<number>();
  #changes = $state(0);
  /** The path of the file on screen as `#changes` last counted it. */
  #path: string | undefined;

  constructor(view: PrFiles | null | undefined) {
    this.#view = view;
  }

  get view(): PrFiles | null | undefined {
    return this.#view;
  }

  set view(view: PrFiles | null | undefined) {
    this.show(view, this.#onScreen);
  }

  get onScreen(): number | undefined {
    return this.#onScreen;
  }

  set onScreen(file: number | undefined) {
    this.show(this.#view, file);
  }

  get screenChanges(): number {
    return this.#changes;
  }

  /** Shows the diff `view` with the file at `file` among its files on screen, as a newer diff that lists
   * the same file elsewhere does at once. */
  show(view: PrFiles | null | undefined, file: number | undefined) {
    this.#view = view;
    this.#onScreen = file;
    const path = file === undefined || view?.state !== "ready" ? undefined : view.files[file]?.path;
    if (path === this.#path) return;
    this.#path = path;
    this.#changes += 1;
  }
}

/** A Files tab with the diff `view`, the core's answer, and no file on screen yet. */
export const treeSource = (view: PrFiles | null | undefined) => new FakeTreeSource(view);
