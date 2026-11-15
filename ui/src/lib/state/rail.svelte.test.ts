import { createRawSnippet } from "svelte";
import { expect, it } from "vitest";
import { RAIL_ORDER, RAIL_TAB_OF, railTabLabel, RailState } from "./rail.svelte";

/** Two tabs, as the rail has once #413 adds Review beside Files. */
type Tab = "first" | "second";

it("opens on a tab's button or key, moves to another tab's, and closes on the tab on show's", () => {
  const rail = new RailState<Tab>("first", "second");
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

it("goes along the tabs on Home, End, ← and →, round from either end, and stays put with one tab", () => {
  const rail = new RailState<"first" | "middle" | "last">("first", "middle", "last");
  const went = (["next", "next", "next", "previous", "first", "previous", "last", "first"] as const).map(
    (step) => {
      rail.go(step);
      return rail.tab;
    },
  );
  const alone = new RailState<Tab>("second");
  alone.go("previous");

  expect([went, rail.open, rail.order]).toEqual([
    ["middle", "last", "first", "last", "first", "last", "last", "first"],
    true,
    ["first", "middle", "last"],
  ]);
  expect([alone.tab, alone.open]).toEqual(["second", true]);
});

it("reopens on the tab picked last, however it closed", () => {
  const rail = new RailState<Tab>("first", "second");
  rail.toggle("first");
  rail.go("next");
  const picked = [rail.open, rail.tab];
  rail.close();
  const closed = rail.open;

  rail.reopen();

  expect([picked, closed]).toEqual([[true, "second"], false]);
  expect([rail.open, rail.tab]).toEqual([true, "second"]);
});

it("puts a tab's count in its label", () => {
  const pane = createRawSnippet(() => ({ render: () => "<p></p>" }));
  const tab = { name: "Review", action: "toggle_rail_files", pane } as const;

  expect([railTabLabel(tab), railTabLabel({ ...tab, count: 3 }), railTabLabel({ ...tab, count: 0 })]).toEqual(
    ["Review", "Review · 3", "Review · 0"],
  );
});

it("orders the tabs as written, and finds the tab an action opens", () => {
  expect([RAIL_ORDER, RAIL_TAB_OF.toggle_rail_files, RAIL_TAB_OF.archive]).toEqual([
    ["files"],
    "files",
    undefined,
  ]);
});
