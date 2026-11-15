import { expect, it } from "vitest";
import { railTabLabel, railTabOf, RailState } from "./rail.svelte";

/** Two tabs, as the rail has once #413 adds Review beside Files. */
type Tab = "first" | "second";

it("opens on a tab's button or key, moves to another tab's, and closes on the tab on show's", () => {
  const rail = new RailState<Tab>("first");
  const before = [rail.open, rail.tab];

  rail.toggle("second");
  const opened = [rail.open, rail.tab];
  rail.toggle("first");
  const moved = [rail.open, rail.tab];
  rail.toggle("first");

  expect([before, opened, moved]).toEqual([
    [false, "first"],
    [true, "second"],
    [true, "first"],
  ]);
  expect([rail.open, rail.tab]).toEqual([false, "first"]);
});

it("reopens on the tab picked last, however it closed", () => {
  const rail = new RailState<Tab>("first");
  rail.toggle("first");
  // ← → in the tab row pick a tab without closing the rail.
  rail.pick("second");
  rail.pick("second");
  const picked = [rail.open, rail.tab];
  rail.close();
  const closed = rail.open;

  rail.reopen();

  expect([picked, closed]).toEqual([[true, "second"], false]);
  expect([rail.open, rail.tab]).toEqual([true, "second"]);
});

it("puts a tab's count in its label", () => {
  expect([railTabLabel("Review"), railTabLabel("Review", 3), railTabLabel("Review", 0)]).toEqual([
    "Review",
    "Review · 3",
    "Review · 0",
  ]);
});

it("finds the tab an action opens", () => {
  expect([railTabOf("toggle_rail_files")?.id, railTabOf("archive")]).toEqual(["files", undefined]);
});
