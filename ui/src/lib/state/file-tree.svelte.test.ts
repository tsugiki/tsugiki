import { expect, it } from "vitest";
import { treedFiles as diff } from "$lib/testing/fixtures";
import { treeSource } from "$lib/testing/tree-source.svelte";
import { FileTreeState } from "./file-tree.svelte";
import type { PrFiles } from "./pr-files.svelte";

/** A Files tab whose diff and file on screen a test changes, and the tree over it. */
const tree = (view: PrFiles | null | undefined) => {
  const files = treeSource(view);
  return { files, tree: new FileTreeState(files) };
};

/** Each row that shows: a folder's name with ▸ or ▾, or a file's name. */
const shown = (state: FileTreeState) =>
  state.rows.map((row) =>
    row.kind === "folder" ? `${row.folded ? "▸" : "▾"} ${row.row.name}` : row.row.name,
  );

it("shows each folder open before what it holds, and each file with its diff's file", () => {
  const { tree: state } = tree(diff());

  expect(shown(state)).toEqual(["▾ Lib", "ast.py", "▾ test", "test_ast.py", "test_re.py", "README.rst"]);
  expect(state.rows.map((row) => row.at)).toEqual([0, 1, 2, 3, 4, 5]);
  const last = state.rows[5];
  expect(last?.kind === "file" && last.file.path).toBe("README.rst");
});

it("says why there are no rows: the diff isn't ready, changes no file, or the filter keeps none", () => {
  const { files, tree: state } = tree(undefined);

  expect(state.note).toBe("Reading the diff…");
  expect(state.rows).toEqual([]);
  files.view = null;
  expect(state.note).toBe("Tsugiki doesn't hold this PR.");
  files.view = { ...diff(), files: [], tree: { rows: [], folded: false } };
  expect(state.note).toBe("This PR changes no files.");
  files.view = diff();
  expect(state.note).toBeUndefined();
  state.type("nothing");
  expect(state.note).toBe("No file's path holds “nothing”.");
});

it("starts every folder folded past a few hundred files, and → and ← unfold and fold the one under the cursor", () => {
  const { tree: state } = tree(diff(true));

  expect(shown(state)).toEqual(["▸ Lib", "README.rst"]);
  state.unfold();
  expect(shown(state)).toEqual(["▾ Lib", "ast.py", "▸ test", "README.rst"]);
  // On an open folder, → goes to its first row.
  state.unfold();
  expect(state.cursor).toBe(1);
  // On a file, → does nothing.
  state.unfold();
  expect(state.cursor).toBe(1);
  // On a file, ← goes to its folder, and on the open folder folds it.
  state.fold();
  expect(state.cursor).toBe(0);
  state.fold();
  expect(shown(state)).toEqual(["▸ Lib", "README.rst"]);
  // On a folded folder at the top, ← stays.
  state.fold();
  expect(state.cursor).toBe(0);
});

it("Enter or a click opens a file, and flips a folder's fold", () => {
  const { tree: state } = tree(diff());

  state.moveTo(2);
  expect(state.pick()).toBeUndefined();
  expect(shown(state)).toEqual(["▾ Lib", "ast.py", "▸ test", "README.rst"]);
  state.moveTo(3);
  expect(state.pick()).toBe(3);
  state.moveTo(2);
  expect(state.pick()).toBeUndefined();
  expect(shown(state)).toHaveLength(6);
});

it("moves the cursor within the rows, and does nothing on a tree with none", () => {
  const { tree: state } = tree(diff());
  const { tree: none } = tree(undefined);

  state.moveTo(99);
  expect(state.cursor).toBe(5);
  state.moveTo(-3);
  expect(state.cursor).toBe(0);
  none.moveTo(1);
  none.fold();
  none.unfold();
  expect(none.pick()).toBeUndefined();
  expect(none.cursor).toBe(0);
});

it("filters the files by path whatever the case, with their folders open, and keeps the folds for after", () => {
  const { tree: state } = tree(diff(true));

  state.type("TEST_");
  expect(shown(state)).toEqual(["▾ Lib", "▾ test", "test_ast.py", "test_re.py"]);
  // A fold while filtering lasts as long as the text.
  state.moveTo(1);
  state.fold();
  expect(shown(state)).toEqual(["▾ Lib", "▸ test"]);
  state.type("test_r");
  expect(shown(state)).toEqual(["▾ Lib", "▾ test", "test_re.py"]);
  state.type("nothing");
  expect(state.rows).toEqual([]);
  state.type("");
  expect(shown(state)).toEqual(["▸ Lib", "README.rst"]);
});

it("marks the file on screen, or the folded folder it sits in, and nothing while the filter hides it", () => {
  const { files, tree: state } = tree(diff());

  expect(state.current).toBeUndefined();
  files.onScreen = 2;
  expect(state.current).toBe(4);
  state.moveTo(2);
  state.fold();
  expect(state.current).toBe(2);
  files.onScreen = 0;
  expect(state.current).toBe(1);
  state.type("README");
  expect(state.current).toBeUndefined();
  // Nor the folded folder it sits in, which shows for the files the filter keeps.
  files.onScreen = 1;
  state.type("test_r");
  state.moveTo(1);
  state.fold();
  expect([shown(state), state.current]).toEqual([["▾ Lib", "▸ test"], undefined]);
  // A file past the tree's, as from an older diff, is nowhere in it.
  state.type("");
  files.onScreen = 9;
  expect(state.current).toBeUndefined();
});

it("keeps the cursor where the user put it until another file comes on screen, which it follows", () => {
  const { files, tree: state } = tree(diff());
  files.onScreen = 1;

  expect(state.cursor).toBe(3);
  state.moveTo(0);
  state.unfold();
  expect(state.cursor).toBe(1);
  files.onScreen = 3;
  expect(state.cursor).toBe(5);
  // A cursor on a row that no longer shows goes to the file on screen, or the folded folder it sits in.
  state.moveTo(3);
  files.view = diff(true);
  expect(state.cursor).toBe(1);
  files.onScreen = 0;
  expect(state.cursor).toBe(0);
});

it("follows the file on screen once the user scrolls back from the file they opened to the one before", () => {
  const { files, tree: state } = tree(diff());
  files.onScreen = 0;
  state.moveTo(4);

  expect(state.pick()).toBe(2);
  files.onScreen = 2;
  files.onScreen = 0;
  expect(state.cursor).toBe(1);
});

it("keeps the cursor where the user put it while the file on screen is one the filter hides", () => {
  const { files, tree: state } = tree(diff());
  files.onScreen = 1;
  state.type("test_");
  state.moveTo(1);

  files.onScreen = 0;
  const hidden = state.cursor;
  files.onScreen = 2;

  expect([hidden, state.cursor]).toEqual([1, 3]);
});
