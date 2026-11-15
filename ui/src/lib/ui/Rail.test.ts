import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { createRawSnippet, tick } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { remapKeys } from "$lib/state/keymap.svelte";
import { RailState, type RailTab } from "$lib/state/rail.svelte";
import Rail from "./Rail.svelte";

/** Two test tabs, as the rail has once #413 adds Review beside Files, the second with a count. */
type Tab = "files" | "review";

const pane = (html: string) => createRawSnippet(() => ({ render: () => html }));
const TABS: Record<Tab, RailTab> = {
  files: { name: "Files", action: "toggle_rail_files", pane: pane("<button>Tree</button>") },
  // Review borrows Pick a Theme's key, `Ctrl+K T`.
  review: { name: "Review", action: "pick_theme", count: 3, pane: pane("<p>Drafts</p>") },
};

afterEach(() => {
  remapKeys({});
});

const shown = (order: [Tab, ...Tab[]], tab: Tab = order[0]) => {
  const rail = new RailState<Tab>(...order);
  rail.pick(tab);
  const onleave = vi.fn();
  const { unmount } = render(Rail, { tabs: TABS, rail, edge: pane("<div></div>"), onleave });
  return { rail, onleave, unmount };
};

const tab = (name: string) => screen.getByRole("tab", { name });

it("shows a row of tabs with two tabs, the one picked last on show, and its pane under them", () => {
  remapKeys({ pick_theme: "" });
  shown(["files", "review"], "review");

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
  const { rail } = shown(["files", "review"], "review");
  const onReview = screen.getByRole("button", { name: "Close the rail" }).title;

  await fireEvent.click(tab("Files"));
  const moved = [
    rail.open,
    rail.tab,
    screen.getByRole("tabpanel").textContent,
    tab("Review · 3").textContent,
  ];
  const onFiles = screen.getByRole("button", { name: "Close the rail" }).title;
  await fireEvent.click(tab("Files"));

  expect([moved, onReview, onFiles]).toEqual([
    [true, "files", "Tree", "Review · 3"],
    "Close (Ctrl+K T)",
    "Close (Ctrl+K F)",
  ]);
  expect([rail.open, rail.tab]).toEqual([false, "files"]);
});

it("moves between the tabs on ← and →, round from either end, and takes the focus along", async () => {
  const { rail } = shown(["files", "review"]);
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

it("gives the focus in a pane to the tab on show as a key shows another tab, and still holds it", async () => {
  const { rail, onleave, unmount } = shown(["files", "review"]);
  screen.getByRole("button", { name: "Tree" }).focus();

  rail.toggle("review");
  await tick();
  const focused = document.activeElement === tab("Review · 3");
  // WebView2 says the focus left, with nowhere it went, as the pane it was in goes; jsdom doesn't.
  tab("Review · 3").dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: null }));
  await Promise.resolve();
  unmount();
  await Promise.resolve();

  expect(focused).toBe(true);
  // The rail held the focus as it went, so it hands it on.
  expect(onleave).toHaveBeenCalledOnce();
});

it("names each tab's key as `[keys]` gives it", async () => {
  shown(["files", "review"]);

  remapKeys({ toggle_rail_files: "ctrl+j f" });
  await tick();

  expect([tab("Files").title, screen.getByRole("button", { name: "Close the rail" }).title]).toEqual([
    "Files (Ctrl+J F)",
    "Close (Ctrl+J F)",
  ]);
});

it("closes on ×", async () => {
  const { rail } = shown(["files", "review"]);

  await fireEvent.click(screen.getByRole("button", { name: "Close the rail" }));

  expect(rail.open).toBe(false);
});

it("shows no tab row with one tab, but its name, count and all, over its pane", () => {
  shown(["review"]);

  expect(screen.queryByRole("tablist")).toBeNull();
  expect(screen.getByRole("heading").textContent).toBe("Review · 3");
  expect(screen.getByRole("region", { name: "Review" }).textContent).toBe("Drafts");
});
