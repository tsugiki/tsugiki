import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { ok, refused } from "$lib/testing/fixtures";
import { UnstoredViewState } from "$lib/state/unstored-view.svelte";
import UnstoredDetail from "./UnstoredDetail.svelte";

const core = vi.hoisted(() => ({ fetchItem: vi.fn(), openOnGithub: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({ commands: core }));

beforeEach(() => {
  vi.resetAllMocks();
});

/** The pane for a fetch of #12 that `answer` answered, once it has. */
async function failedWith(answer: object) {
  core.fetchItem.mockResolvedValue(answer);
  const view = new UnstoredViewState("python/cpython", 12);
  await view.load(() => {});
  render(UnstoredDetail, { view });
}

it("names the number loading, and takes the focus as the list hides", () => {
  core.fetchItem.mockReturnValue(new Promise(() => {}));
  const view = new UnstoredViewState("python/cpython", 12);
  void view.load(() => {});

  render(UnstoredDetail, { view, focusable: true });

  expect(screen.getByRole("heading", { name: "python/cpython #12" })).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe("Fetching python/cpython #12 from GitHub…");
  expect(screen.getByRole("region", { name: "Detail pane" }).tabIndex).toBe(-1);
  expect(screen.queryByRole("button", { name: "Open in browser" })).toBeNull();
});

it.each([
  ["missing", "GitHub has no Issue or PR #12 in python/cpython."],
  ["hidden", "Your token can't see #12 in python/cpython."],
  ["offline", "Tsugiki doesn't hold #12, and can't ask GitHub for it now. The status bar says why."],
  ["other", "Tsugiki couldn't fetch #12 from GitHub. tsugiki.log says why."],
] as const)("says why a fetch that brought nothing as %s did", async (why, words) => {
  await failedWith(ok({ kind: "unfetched", why }));

  expect(screen.getByRole("alert").textContent).toBe(words);
  expect(screen.getByRole("button", { name: "Open in browser" })).toBeTruthy();
});

it("says what refused the fetch", async () => {
  await failedWith(refused("Not a Repository name."));

  expect(screen.getByRole("alert").textContent).toBe("Tsugiki couldn't fetch #12: Not a Repository name.");
});

it("says what kept the browser from opening the number", async () => {
  core.openOnGithub.mockResolvedValue(refused("No browser."));
  await failedWith(ok({ kind: "unfetched", why: "missing" }));

  await fireEvent.click(screen.getByRole("button", { name: "Open in browser" }));

  expect((await screen.findAllByRole("alert")).map((alert) => alert.textContent)).toEqual([
    "GitHub has no Issue or PR #12 in python/cpython.",
    "No browser.",
  ]);
});
