import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { FileTreeState } from "$lib/state/file-tree.svelte";
import type { PrFiles } from "$lib/state/pr-files.svelte";
import type { PrViewState } from "$lib/state/pr-view.svelte";
import { diffFile, fileRow, folderRow, readyFiles, treedFiles } from "$lib/testing/fixtures";
import { pressOnFocus } from "$lib/testing/helpers";
import { treeSource } from "$lib/testing/tree-source.svelte";
import RailFiles from "./RailFiles.svelte";

/** A row's height in pixels, as the tab draws it. */
const ROW = 22;

/** The tab in a PR view whose Files tab holds `view`, and what it went to. */
const drawn = (view: PrFiles | null | undefined) => {
  const files = treeSource(view);
  const pr = { tree: new FileTreeState(files), loadFiles: vi.fn(), goToFile: vi.fn() };
  render(RailFiles, { pr: pr as unknown as PrViewState });
  return { files, pr };
};

const box = () => screen.getByRole("textbox", { name: "Filter the changed files" });
const list = () => screen.getByRole("tree", { name: "Changed files" });
const items = () => within(list()).getAllByRole("treeitem");
const texts = () => items().map((item) => item.textContent?.replace(/\s+/g, " ").trim());
const cursorText = () =>
  within(list()).getByRole("treeitem", { selected: true }).textContent?.replace(/\s+/g, " ").trim();

afterEach(() => {
  cleanup();
});

it("reads the PR's diff and shows its files as a tree, each file with its counts, threads and drafts", () => {
  const view = readyFiles(9, {
    files: [
      diffFile("Lib/ast.py", 0, { change: { kind: "lines", added: 12, removed: 3 } }),
      diffFile("Lib/old.py", 3, { old_path: "Lib/older.py" }),
      diffFile("Python/gen.c.h", 6, { generated: true, change: { kind: "binary" } }),
    ],
    tree: {
      rows: [
        folderRow("Lib", "Lib", 0, 3),
        fileRow(0, "ast.py", 1, { threads: 2, drafts: 1 }),
        fileRow(1, "old.py", 1, { threads: 1 }),
        folderRow("Python", "Python", 0, 5),
        fileRow(2, "gen.c.h", 1),
      ],
      folded: false,
    },
  });
  const { pr } = drawn(view);

  expect(pr.loadFiles).toHaveBeenCalled();
  expect(texts()).toEqual(["▾Lib", "ast.py21+12−3", "old.py1+1−0", "▾Python", "gen.c.hGeneratedbinary"]);
  const titles = (selector: string) =>
    [...list().querySelectorAll<HTMLElement>(selector)].map((each) => each.title);
  expect(titles(".rail-count")).toEqual(["2 review threads", "1 draft", "1 review thread"]);
  expect(items().map((item) => item.title)).toEqual([
    "Lib",
    "Lib/ast.py",
    "Lib/older.py → Lib/old.py",
    "Python",
    "Python/gen.c.h",
  ]);
  expect(items()[4]?.classList.contains("generated")).toBe(true);
  expect(items().map((item) => item.getAttribute("aria-level"))).toEqual(["1", "2", "2", "1", "2"]);
  expect(items()[0]?.getAttribute("aria-expanded")).toBe("true");
});

it("says why it has no tree: the diff isn't ready, changes no file, or the filter keeps none", async () => {
  const { files } = drawn(undefined);

  expect(screen.getByText("Reading the diff…")).toBeDefined();
  expect(screen.queryByRole("tree")).toBeNull();
  files.view = readyFiles(0);
  await tick();
  expect(screen.getByText("This PR changes no files.")).toBeDefined();
  files.view = treedFiles();
  await tick();
  expect(document.querySelector(".rail-note")).toBeNull();
  await fireEvent.input(box(), { target: { value: "spam" } });

  expect(screen.getByText("No file's path holds “spam”.")).toBeDefined();
  expect(screen.queryByRole("tree")).toBeNull();
});

it("narrows the tree to the files whose path holds what the user types", async () => {
  drawn(treedFiles());

  await fireEvent.input(box(), { target: { value: "test_r" } });

  expect(texts()).toEqual(["▾Lib", "▾test", "test_re.py+1−0"]);
});

it("opens a file on a click, and folds or unfolds a folder", async () => {
  const { pr } = drawn(treedFiles());

  await fireEvent.click(items()[2] as HTMLElement);
  expect(texts()).toEqual(["▾Lib", "ast.py+1−0", "▸test", "README.rst+1−0"]);
  await fireEvent.click(items()[3] as HTMLElement);

  expect(pr.goToFile).toHaveBeenCalledWith(3);
  expect(pr.goToFile).toHaveBeenCalledOnce();
});

it("goes from the box to the list on ↓, where the keys move, fold, unfold and open, and go no further", async () => {
  const { pr } = drawn(treedFiles(true));
  const heard = vi.fn();
  window.addEventListener("keydown", heard);
  box().focus();

  await pressOnFocus("ArrowDown");
  expect(document.activeElement).toBe(list());
  await pressOnFocus("ArrowRight");
  expect(texts()).toEqual(["▾Lib", "ast.py+1−0", "▸test", "README.rst+1−0"]);
  await pressOnFocus("ArrowRight");
  expect(cursorText()).toBe("ast.py+1−0");
  await pressOnFocus("End");
  expect(cursorText()).toBe("README.rst+1−0");
  await pressOnFocus("Home");
  await pressOnFocus("PageDown");
  expect(cursorText()).toBe("ast.py+1−0");
  await pressOnFocus("PageUp");
  await pressOnFocus("ArrowDown");
  await pressOnFocus("ArrowDown");
  await pressOnFocus("Enter");
  expect(texts()).toHaveLength(6);
  await pressOnFocus("ArrowDown");
  await pressOnFocus("Enter");
  expect(pr.goToFile).toHaveBeenCalledWith(1);
  await pressOnFocus("ArrowUp");
  await pressOnFocus("ArrowLeft");
  expect(texts()).toHaveLength(4);
  // A key the list doesn't take goes on, and so does a modifier alone.
  await pressOnFocus("a");
  await pressOnFocus("Shift");
  window.removeEventListener("keydown", heard);

  expect(heard).toHaveBeenCalledTimes(2);
  expect(list().getAttribute("aria-activedescendant")).toBe(items()[2]?.id);
});

it("leaves ↓ in the box alone while the tree shows no row", async () => {
  drawn(treedFiles());
  await fireEvent.input(box(), { target: { value: "spam" } });
  box().focus();

  await fireEvent.keyDown(box(), { key: "ArrowDown" });
  await fireEvent.keyDown(box(), { key: "a" });

  expect(document.activeElement).toBe(box());
});

it("marks the file on screen in the Files tab, and scrolls the list to keep it in view", async () => {
  const many = 1_118;
  const view = readyFiles(many, {
    files: Array.from({ length: many }, (_, at) => diffFile(`Lib/test_${String(at)}.py`, at)),
    tree: {
      rows: [
        folderRow("Lib", "Lib", 0, many + 1),
        ...Array.from({ length: many }, (_, at) => fileRow(at, `test_${String(at)}.py`, 1)),
      ],
      folded: true,
    },
  });
  const { files, pr } = drawn(view);

  // Past a few hundred files the folders start folded.
  expect(texts()).toEqual(["▸Lib"]);
  files.onScreen = 900;
  await tick();
  expect(items()[0]?.getAttribute("aria-current")).toBe("true");
  pr.tree.unfold();
  await tick();

  const current = list().querySelector<HTMLElement>('[aria-current="true"]');
  expect(current?.textContent).toContain("test_900.py");
  expect(list().scrollTop).toBe(901 * ROW);
  // The list draws only the rows near what shows, and names the cursor's row, which stays on the folder the
  // user unfolded, only while it draws it.
  expect(items().length).toBeLessThan(40);
  expect(list().getAttribute("aria-activedescendant")).toBeNull();
  list().scrollTop = 0;
  await fireEvent.scroll(list());
  expect(texts()[0]).toBe("▾Lib");
  expect(list().getAttribute("aria-activedescendant")).toBe(items()[0]?.id);
});
