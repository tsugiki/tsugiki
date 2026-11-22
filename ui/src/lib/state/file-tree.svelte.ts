// The rail's Files tab (#604): the PR's changed files as a tree of folders, which the core builds with the
// file list. This only folds and filters its rows: a folder folds to its own row, past a few hundred files
// every folder starts folded, and a filter keeps the files whose path holds what the user typed, with the
// folders they sit in, unfolded. The keys move a cursor through the rows, fold and unfold a folder, and
// open a file. The file on screen in the Files tab is marked, or the folded folder it sits in, and the
// cursor follows it whenever another file comes on screen.
import { SvelteMap } from "svelte/reactivity";
import type { FileTree, TreeRow } from "$lib/data/bindings";
import type { DiffFile, PrFilesState } from "./pr-files.svelte";
import { rowWithin } from "./rail.svelte";
import { searchDiff, type SearchDiff } from "./text-search.svelte";

export type { FileTree, TreeRow };

/** A row of the tree as it shows, with its place among the tree's rows: a folder, with whether it is
 * folded, or a file of the diff. */
export type ShownRow =
  | { kind: "folder"; row: Extract<TreeRow, { kind: "folder" }>; at: number; folded: boolean }
  | { kind: "file"; row: Extract<TreeRow, { kind: "file" }>; at: number; file: DiffFile };

/** What the tree reads of the Files tab: its diff, or why it has none, and the file it has on screen. */
export type TreeSource = Pick<PrFilesState, "view" | "error" | "unfetched" | "waitsForNetwork" | "onScreen">;

/** A row by what it shows, so the cursor stays on it as the rows around it fold, filter or change. */
type RowKey = { kind: "folder" | "file"; path: string };

/** The tree of a diff, with its files and the place of each file's row among the tree's rows. */
type Tree = { tree: FileTree; files: DiffFile[]; rowOf: number[] };

/** What the tab says while the diff isn't ready. */
const UNREADY: Record<Exclude<SearchDiff["kind"], "ready">, string> = {
  loading: "Reading the diff…",
  offline: "The PR's commits come once GitHub answers.",
  asking: "The diff waits for the Repository's clone, which waits for an answer in the Files tab.",
  absent: "Tsugiki doesn't hold this PR.",
  failed: "The PR's diff didn't load. The Files tab says why.",
};

export class FileTreeState {
  /** What the filter box holds. */
  filter = $state("");
  readonly #files: TreeSource;
  /** Whether the user folded or unfolded each folder they toggled, by its path, which wins over how the
   * folder starts. */
  #chosen = new SvelteMap<string, boolean>();
  /** The same while the filter holds text, which starts every folder it keeps unfolded. A new text starts
   * them so again. */
  #chosenFiltered = new SvelteMap<string, boolean>();
  /** The row the user put the cursor on, and the file on screen at the time: once another file comes on
   * screen, the cursor follows it. */
  #cursorKey = $state.raw<{ key: RowKey; screen: number | undefined }>();

  /** The tree of the diff the Files tab, `files`, holds, and the file it has on screen. */
  constructor(files: TreeSource) {
    this.#files = files;
  }

  /** The tree of the diff on show, once it is ready. */
  readonly #tree: Tree | undefined = $derived.by(() => {
    const view = this.#files.view;
    if (view?.state !== "ready") return undefined;
    const rowOf: number[] = [];
    view.tree.rows.forEach((row, at) => {
      if (row.kind === "file") rowOf[row.file] = at;
    });
    return { tree: view.tree, files: view.files, rowOf };
  });

  /** Why there is no tree yet, while the diff isn't ready. */
  get unready(): string | undefined {
    const diff = searchDiff(this.#files);
    return diff.kind === "ready" ? undefined : UNREADY[diff.kind];
  }

  /** The folds the user chose that apply now: those since the filter's text, while it holds any. */
  get #folds(): SvelteMap<string, boolean> {
    return this.filter === "" ? this.#chosen : this.#chosenFiltered;
  }

  /** The rows that show: each folder, and what it holds unless it is folded, and with a filter only the
   * files whose path holds its text, whatever the case, and the folders they sit in. */
  readonly rows: readonly ShownRow[] = $derived.by(() => {
    const tree = this.#tree;
    if (tree === undefined) return [];
    const rows = tree.tree.rows;
    const kept = this.#kept(tree);
    const startsFolded = this.filter === "" && tree.tree.folded;
    const shown: ShownRow[] = [];
    let at = 0;
    while (at < rows.length) {
      const row = rows[at] as TreeRow;
      const end = row.kind === "folder" ? row.end : at + 1;
      if (kept !== undefined && kept[end] === kept[at]) {
        at = end;
      } else if (row.kind === "file") {
        shown.push({ kind: "file", row, at, file: tree.files[row.file] as DiffFile });
        at = end;
      } else {
        const folded = this.#folds.get(row.path) ?? startsFolded;
        shown.push({ kind: "folder", row, at, folded });
        at = folded ? end : at + 1;
      }
    }
    return shown;
  });

  /** With a filter, how many files it keeps among the tree's rows before each row, and before the end, so
   * the rows from one place to another keep a file if the counts there differ. Nothing without one. */
  #kept({ tree, files }: Tree): number[] | undefined {
    const filter = this.filter.toLowerCase();
    if (filter === "") return undefined;
    const kept = [0];
    let count = 0;
    for (const row of tree.rows) {
      if (row.kind === "file" && files[row.file]?.path.toLowerCase().includes(filter)) count += 1;
      kept.push(count);
    }
    return kept;
  }

  /** The row of the file on screen in the Files tab, or of the folded folder it sits in. Nothing while no
   * file is on screen, or the filter hides it. */
  readonly current: number | undefined = $derived.by(() => {
    const file = this.#files.onScreen;
    const target = file === undefined ? undefined : this.#tree?.rowOf[file];
    if (target === undefined) return undefined;
    const rows = this.rows;
    // The last row that shows at or before the file's: the file's own, or a folded folder that holds it.
    let low = 0;
    let high = rows.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if ((rows[middle] as ShownRow).at <= target) low = middle + 1;
      else high = middle;
    }
    const row = rows[low - 1];
    const holds = row?.kind === "folder" && row.folded && row.row.end > target;
    return row?.at === target || holds ? low - 1 : undefined;
  });

  /** The row the cursor is on, which ↑ and ↓ move: the one the user put it on, until another file comes on
   * screen, and then the row of that file. The first, while neither shows. */
  readonly cursor: number = $derived.by(() => {
    const chosen = this.#cursorKey;
    if (chosen !== undefined && chosen.screen === this.#files.onScreen) {
      const at = this.rows.findIndex((row) => sameKey(keyOf(row), chosen.key));
      if (at !== -1) return at;
    }
    return this.current ?? 0;
  });

  /** Puts the cursor on row `index`, held within the rows. */
  moveTo(index: number) {
    const row = rowWithin(this.rows, index);
    if (row !== undefined) this.#cursorKey = { key: keyOf(row), screen: this.#files.onScreen };
  }

  /** ←: folds the open folder under the cursor, or moves the cursor to the folder that holds its row. */
  fold() {
    const cursor = this.cursor;
    const row = this.rows[cursor];
    if (row === undefined) return;
    if (row.kind === "folder" && !row.folded) {
      this.#folds.set(row.row.path, true);
      return;
    }
    const depth = row.row.depth;
    const parent = this.rows.findLastIndex((other, at) => at < cursor && other.row.depth < depth);
    if (parent !== -1) this.moveTo(parent);
  }

  /** →: unfolds the folded folder under the cursor, or moves the cursor from an open one to its first row. */
  unfold() {
    const row = this.rows[this.cursor];
    if (row?.kind !== "folder") return;
    if (row.folded) this.#folds.set(row.row.path, false);
    else this.moveTo(this.cursor + 1);
  }

  /** Enter or a click on the row under the cursor: a file's place among the diff's files, to open, or else
   * the folder's fold flips. */
  pick(): number | undefined {
    const row = this.rows[this.cursor];
    if (row?.kind === "file") return row.row.file;
    if (row !== undefined) this.#folds.set(row.row.path, !row.folded);
    return undefined;
  }

  /** The filter's text as the user types it, which unfolds every folder it keeps. The cursor goes back to
   * the file on screen. */
  type(text: string) {
    this.filter = text;
    this.#chosenFiltered.clear();
    this.#cursorKey = undefined;
  }
}

const keyOf = (row: ShownRow): RowKey =>
  row.kind === "folder" ? { kind: "folder", path: row.row.path } : { kind: "file", path: row.file.path };

const sameKey = (one: RowKey, other: RowKey) => one.kind === other.kind && one.path === other.path;
