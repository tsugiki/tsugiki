import { expect, it } from "vitest";
import type { WorkbenchView } from "$lib/state/workbench.svelte";
import { currentView } from "./sidebar-view";

const mine: WorkbenchView = { kind: "filter", query: "is:open author:@me" };
const involved: WorkbenchView = { kind: "filter", query: "is:open involves:@me" };

it.each<[string, WorkbenchView, WorkbenchView, "true" | undefined]>([
  ["the Workbench on show", { kind: "whole" }, { kind: "whole" }, "true"],
  ["another view on show", { kind: "review_requests" }, { kind: "whole" }, undefined],
  ["the same Filter on show", mine, { ...mine }, "true"],
  ["another Filter on show", involved, mine, undefined],
  ["a view that isn't a Filter on show", { kind: "whole" }, mine, undefined],
])("marks an entry current for %s", (_, selected, view, current) => {
  expect(currentView(selected, view)).toBe(current);
});
