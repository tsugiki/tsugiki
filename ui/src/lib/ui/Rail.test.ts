import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { createRawSnippet, tick } from "svelte";
import { expect, it, vi } from "vitest";
import { RailState, type RailTab } from "$lib/state/rail.svelte";
import Rail from "./Rail.svelte";

/** Two test tabs, as the rail has once #413 adds Review beside Files, the second with a count. */
type Tab = "files" | "review";

const pane = (html: string) => createRawSnippet(() => ({ render: () => html }));
const FILES: RailTab<Tab> = {
  id: "files",
  name: "Files",
  key: "Ctrl+K F",
  pane: pane("<button>Tree</button>"),
};
const REVIEW: RailTab<Tab> = {
  id: "review",
  name: "Review",
  count: 3,
  key: null,
  pane: pane("<p>Drafts</p>"),
};

const shown = (tabs: readonly [RailTab<Tab>, ...RailTab<Tab>[]], tab: Tab = "files") => {
  const rail = new RailState<Tab>(tab);
  rail.open = true;
  const onleave = vi.fn();
  render(Rail, { tabs, rail, edge: pane("<div></div>"), onleave });
  return { rail, onleave };
};

const tab = (name: string) => screen.getByRole("tab", { name });

it("shows a row of tabs with two tabs, the one picked last on show, and its pane under them", () => {
  shown([FILES, REVIEW], "review");

  const row = screen.getByRole("tablist", { name: "Rail tabs" });
  expect(
    within(row)
      .getAllByRole("tab")
      .map((each) => [each.textContent, each.title]),
  ).toEqual([
    ["Files", "Files (Ctrl+K F)"],
    ["Review · 3", "Review · 3"],
  ]);
  expect([
    tab("Review · 3").getAttribute("aria-selected"),
    tab("Review · 3").tabIndex,
    tab("Files").tabIndex,
  ]).toEqual(["true", 0, -1]);
  const panel = screen.getByRole("tabpanel", { name: "Review · 3" });
  expect(panel.textContent).toBe("Drafts");
  // × closes the rail on the key of the tab on show, which has none here.
  expect(screen.getByRole("button", { name: "Close the rail" }).title).toBe("Close");
});

it("shows another tab on a click, keeps a tab's count while another shows, and closes on the tab on show", async () => {
  const { rail } = shown([FILES, REVIEW], "review");

  await fireEvent.click(tab("Files"));
  const moved = [
    rail.open,
    rail.tab,
    screen.getByRole("tabpanel").textContent,
    tab("Review · 3").textContent,
  ];
  const close = screen.getByRole("button", { name: "Close the rail" }).title;
  await fireEvent.click(tab("Files"));

  expect([moved, close]).toEqual([[true, "files", "Tree", "Review · 3"], "Close (Ctrl+K F)"]);
  expect([rail.open, rail.tab]).toEqual([false, "files"]);
});

it("moves between the tabs on ← and →, round from either end, and takes the focus along", async () => {
  const { rail } = shown([FILES, REVIEW]);
  tab("Files").focus();

  await fireEvent.keyDown(tab("Files"), { key: "ArrowRight" });
  const right = [rail.tab, document.activeElement === tab("Review · 3")];
  await fireEvent.keyDown(tab("Review · 3"), { key: "ArrowRight" });
  const round = [rail.tab, document.activeElement === tab("Files")];
  await fireEvent.keyDown(tab("Files"), { key: "ArrowLeft" });
  const left = [rail.tab, document.activeElement === tab("Review · 3")];
  // Another key, or an arrow with Ctrl, leaves the tab on show.
  await fireEvent.keyDown(tab("Review · 3"), { key: "ArrowLeft", ctrlKey: true });
  await fireEvent.keyDown(tab("Review · 3"), { key: "a" });

  expect([right, round, left]).toEqual([
    ["review", true],
    ["files", true],
    ["review", true],
  ]);
  expect([rail.open, rail.tab]).toEqual([true, "review"]);
});

it("gives the focus in a pane to the tab on show as a key shows another tab", async () => {
  const { rail } = shown([FILES, REVIEW]);
  screen.getByRole("button", { name: "Tree" }).focus();

  rail.toggle("review");
  await tick();

  expect(document.activeElement).toBe(tab("Review · 3"));
});

it("closes on ×", async () => {
  const { rail } = shown([FILES, REVIEW]);

  await fireEvent.click(screen.getByRole("button", { name: "Close the rail" }));

  expect(rail.open).toBe(false);
});

it("shows no tab row with one tab, but its name, count and all, over its pane", () => {
  shown([{ ...REVIEW, id: "files" }]);

  expect(screen.queryByRole("tablist")).toBeNull();
  expect(screen.getByRole("heading").textContent).toBe("Review · 3");
  expect(screen.getByRole("region", { name: "Review" }).textContent).toBe("Drafts");
});

it("shows the first tab while the tabs leave out the one picked last", () => {
  shown([FILES], "review");

  expect(screen.getByRole("region", { name: "Files" }).textContent).toBe("Tree");
});
