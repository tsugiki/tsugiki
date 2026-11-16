import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import Sidebar from "./Sidebar.svelte";

const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 3, problem: null };
const involved = {
  name: "Involved",
  query: "is:open involves:@me",
  on_workbench: false,
  count: 9,
  problem: null,
};

const whole = { kind: "whole" } as const;
const button = (name: string) => screen.getByRole("button", { name });
/** The open Repository, for the tests that don't look at it. */
const cpython = { repository: "python/cpython", onpick: () => {}, onsettings: () => {}, onicons: () => {} };

it("starts with the open Repository's name, and a click on it asks for the Repository picker", async () => {
  const onpick = vi.fn();
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [],
    selected: whole,
    onselect: () => {},
    ...cpython,
    onpick,
  });
  const name = screen.getAllByRole("button")[0];
  expect(name).toBe(button("python/cpython"));
  expect(name?.getAttribute("aria-haspopup")).toBe("dialog");
  await fireEvent.click(button("python/cpython"));
  expect(onpick).toHaveBeenCalledOnce();
});

it("has ⚙ beside the Repository's name, and a click on it asks for the Settings dialog", async () => {
  const onsettings = vi.fn();
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [],
    selected: whole,
    onselect: () => {},
    ...cpython,
    onsettings,
  });
  const settings = button("Settings");
  expect(screen.getAllByRole("button")[1]).toBe(settings);
  expect(settings.textContent).toBe("⚙");
  expect(settings.getAttribute("aria-haspopup")).toBe("dialog");
  await fireEvent.click(settings);
  expect(onsettings).toHaveBeenCalledOnce();
});

it("has « after ⚙, and a click on it asks for the Sidebar as icons", async () => {
  const onicons = vi.fn();
  render(Sidebar, {
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [],
    selected: whole,
    onselect: () => {},
    ...cpython,
    onicons,
  });
  const fold = button("Sidebar as Icons");
  expect([screen.getAllByRole("button")[2], fold.textContent, fold.title]).toEqual([
    fold,
    "«",
    "Sidebar as Icons",
  ]);
  await fireEvent.click(fold);
  expect(onicons).toHaveBeenCalledOnce();
});

it("names ⚙'s key in its tooltip as the OS calls it", () => {
  const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  const props = {
    count: 0,
    reviewRequests: 0,
    staleReviews: 0,
    filters: [],
    selected: whole,
    onselect: () => {},
  };
  const { unmount } = render(Sidebar, { ...props, ...cpython });
  expect(button("Settings").title).toBe("Settings (Cmd+,)");
  unmount();

  platform.mockReturnValue("Win32");
  render(Sidebar, { ...props, ...cpython });
  expect(button("Settings").title).toBe("Settings (Ctrl+,)");
  platform.mockRestore();
});

it("lists the Workbench and each Filter with its count", () => {
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine, involved],
    selected: whole,
    onselect: () => {},
  });
  expect(screen.getAllByRole("button").map((line) => line.textContent.replace(/\s+/g, " ").trim())).toEqual([
    "python/cpython",
    "⚙",
    "«",
    "Workbench 12",
    "Review requests 4",
    "Stale reviews 2",
    "Mine 3",
    "Involved 9",
  ]);
});

it("heads the built-in views, and the Filters while there are any", () => {
  const headings = () => screen.queryAllByRole("heading").map((heading) => heading.textContent);
  const { unmount } = render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine],
    selected: whole,
    onselect: () => {},
  });
  expect(headings()).toEqual(["Built-in views", "Filters"]);
  unmount();

  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [],
    selected: whole,
    onselect: () => {},
  });
  expect(headings()).toEqual(["Built-in views"]);
});

it("marks the Workbench while nothing else is selected", () => {
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine],
    selected: whole,
    onselect: () => {},
  });
  expect(button("Workbench 12").getAttribute("aria-current")).toBe("true");
  expect(button("Mine 3").getAttribute("aria-current")).toBeNull();
});

it("marks the Filter selected", () => {
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
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
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine],
    selected: { kind: "review_requests" },
    onselect: () => {},
  });
  expect(button("Review requests 4").getAttribute("aria-current")).toBe("true");
  expect(button("Stale reviews 2").getAttribute("aria-current")).toBeNull();
  expect(button("Workbench 12").getAttribute("aria-current")).toBeNull();
  expect(button("Mine 3").getAttribute("aria-current")).toBeNull();
});

it("marks Stale reviews selected", () => {
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine],
    selected: { kind: "stale_reviews" },
    onselect: () => {},
  });
  expect(button("Stale reviews 2").getAttribute("aria-current")).toBe("true");
  expect(button("Review requests 4").getAttribute("aria-current")).toBeNull();
  expect(button("Workbench 12").getAttribute("aria-current")).toBeNull();
});

it("says which line the user picks", async () => {
  const onselect = vi.fn();
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [mine],
    selected: whole,
    onselect,
  });
  await fireEvent.click(button("Mine 3"));
  expect(onselect).toHaveBeenLastCalledWith({ kind: "filter", query: mine.query });
  await fireEvent.click(button("Review requests 4"));
  expect(onselect).toHaveBeenLastCalledWith({ kind: "review_requests" });
  await fireEvent.click(button("Stale reviews 2"));
  expect(onselect).toHaveBeenLastCalledWith({ kind: "stale_reviews" });
  await fireEvent.click(button("Workbench 12"));
  expect(onselect).toHaveBeenLastCalledWith(whole);
});

it("lists two Filters that share a search", () => {
  render(Sidebar, {
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
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
    ...cpython,
    count: 12,
    reviewRequests: 4,
    staleReviews: 2,
    filters: [{ ...mine, problem }],
    selected: whole,
    onselect: () => {},
  });
  expect(screen.getByText(problem)).toBeTruthy();
});
