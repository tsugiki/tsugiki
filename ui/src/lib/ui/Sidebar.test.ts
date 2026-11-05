import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import Sidebar from "./Sidebar.svelte";

const mine = { name: "Mine", query: "is:open author:@me", count: 3, problem: null };
const involved = { name: "Involved", query: "is:open involves:@me", count: 9, problem: null };

const whole = { kind: "whole" } as const;
const button = (name: string) => screen.getByRole("button", { name });

it("lists the Workbench and each Filter with its count", () => {
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    filters: [mine, involved],
    selected: whole,
    onselect: () => {},
  });
  expect(screen.getAllByRole("button").map((line) => line.textContent.replace(/\s+/g, " ").trim())).toEqual([
    "Workbench 12",
    "Review requests 4",
    "Mine 3",
    "Involved 9",
  ]);
});

it("marks the Workbench while nothing else is selected", () => {
  render(Sidebar, { count: 12, reviewRequests: 4, filters: [mine], selected: whole, onselect: () => {} });
  expect(button("Workbench 12").getAttribute("aria-current")).toBe("true");
  expect(button("Mine 3").getAttribute("aria-current")).toBeNull();
});

it("marks the Filter selected", () => {
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    filters: [mine, involved],
    selected: { kind: "filter", query: mine.query },
    onselect: () => {},
  });
  expect(button("Workbench 12").getAttribute("aria-current")).toBeNull();
  expect(button("Mine 3").getAttribute("aria-current")).toBe("true");
  expect(button("Involved 9").getAttribute("aria-current")).toBeNull();
});

it("marks Review requests selected", () => {
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    filters: [mine],
    selected: { kind: "review_requests" },
    onselect: () => {},
  });
  expect(button("Review requests 4").getAttribute("aria-current")).toBe("true");
  expect(button("Workbench 12").getAttribute("aria-current")).toBeNull();
  expect(button("Mine 3").getAttribute("aria-current")).toBeNull();
});

it("says which line the user picks", async () => {
  const onselect = vi.fn();
  render(Sidebar, { count: 12, reviewRequests: 4, filters: [mine], selected: whole, onselect });
  await fireEvent.click(button("Mine 3"));
  expect(onselect).toHaveBeenLastCalledWith({ kind: "filter", query: mine.query });
  await fireEvent.click(button("Review requests 4"));
  expect(onselect).toHaveBeenLastCalledWith({ kind: "review_requests" });
  await fireEvent.click(button("Workbench 12"));
  expect(onselect).toHaveBeenLastCalledWith(whole);
});

it("lists two Filters that share a search", () => {
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    filters: [mine, { ...mine, name: "Also mine" }],
    selected: whole,
    onselect: () => {},
  });
  expect(button("Mine 3")).toBeTruthy();
  expect(button("Also mine 3")).toBeTruthy();
});

it("says what stopped a Filter's last search", () => {
  const problem = "GitHub can't run this search.";
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    filters: [{ ...mine, problem }],
    selected: whole,
    onselect: () => {},
  });
  expect(screen.getByText(problem)).toBeTruthy();
});
