import { fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import SidebarIcons, { badge, initials } from "./SidebarIcons.svelte";

const mine = { name: "Mine", query: "is:open author:@me", on_workbench: true, count: 3, problem: null };
const involved = {
  name: "Involved",
  query: "is:open involves:@me",
  on_workbench: false,
  count: 0,
  problem: null,
};

const whole = { kind: "whole" } as const;
const button = (name: string) => screen.getByRole("button", { name });
/** The strip of a Repository, for the tests that don't look at its callbacks. */
const strip = {
  repository: "python/cpython",
  onfull: () => {},
  onsettings: () => {},
  count: 12,
  reviewRequests: 4,
  staleReviews: 0,
  filters: [mine, involved],
  selected: whole,
  onselect: () => {},
};

it.each([
  ["python/cpython", "CP"],
  ["some-owner/gc-scope", "GS"],
  ["some-owner/pep_848.sim", "P8"],
  ["some-owner/x", "X"],
  ["some-owner/.github", "GI"],
  ["some-owner/_tools", "TO"],
  ["some-owner/---", "--"],
])("takes the initials of %s from the name after its owner", (repository, letters) => {
  expect(initials(repository)).toBe(letters);
});

it.each([
  [0, undefined],
  [7, "7"],
  [99, "99"],
  [100, "99+"],
])("badges a count of %i as %s", (count, words) => {
  expect(badge(count)).toBe(words);
});

it("lists the initials, an icon for each entry with its count and name, then ⚙", () => {
  render(SidebarIcons, strip);

  const buttons = screen.getAllByRole("button");
  expect(
    buttons.map((icon) => [icon.getAttribute("aria-label"), icon.title, icon.textContent.trim()]),
  ).toEqual([
    ["Full Sidebar", "python/cpython", "CP"],
    ["Workbench (12)", "Workbench", "12"],
    ["Review requests (4)", "Review requests", "4"],
    ["Stale reviews", "Stale reviews", ""],
    ["Mine (3)", "Mine", "3"],
    ["Involved", "Involved", ""],
    ["Settings", "Settings (Ctrl+,)", "⚙"],
  ]);
  expect(screen.getByRole("navigation", { name: "Sidebar" })).toBeTruthy();
});

it("marks the view on show", () => {
  render(SidebarIcons, { ...strip, selected: { kind: "filter", query: mine.query } });

  const current = screen
    .getAllByRole("button")
    .filter((icon) => icon.getAttribute("aria-current") === "true");
  expect(current).toEqual([button("Mine (3)")]);
});

it("asks for each entry's view on a click, and for the full Sidebar on a click on the initials", async () => {
  const onselect = vi.fn();
  const onfull = vi.fn();
  const onsettings = vi.fn();
  render(SidebarIcons, { ...strip, onselect, onfull, onsettings });

  for (const name of ["Workbench (12)", "Review requests (4)", "Stale reviews", "Mine (3)"]) {
    await fireEvent.click(button(name));
  }
  await fireEvent.click(button("Full Sidebar"));
  await fireEvent.click(button("Settings"));

  expect(onselect.mock.calls).toEqual([
    [{ kind: "whole" }],
    [{ kind: "review_requests" }],
    [{ kind: "stale_reviews" }],
    [{ kind: "filter", query: mine.query }],
  ]);
  expect([onfull.mock.calls.length, onsettings.mock.calls.length]).toEqual([1, 1]);
});

it("says why a Filter's last search failed, and marks its icon with ! in place of its count", () => {
  const cut = "GitHub stopped the search at 1000 results.";
  render(SidebarIcons, {
    ...strip,
    filters: [
      { ...mine, problem: cut },
      { ...involved, problem: cut },
    ],
  });

  // The count may be short, so the label leaves it out, as the badge and the tooltip do.
  const icons = [button(`Mine. ${cut}`), button(`Involved. ${cut}`)];

  expect(icons.map((icon) => [icon.title, icon.querySelector(".badge.problem")?.textContent])).toEqual([
    [`Mine\n${cut}`, "!"],
    [`Involved\n${cut}`, "!"],
  ]);
  expect(icons.map((icon) => icon.querySelectorAll(".badge").length)).toEqual([1, 1]);
});

it("draws no rule over the Filters while there are none", () => {
  const { container, unmount } = render(SidebarIcons, strip);
  const withFilters = container.querySelectorAll(".rule").length;
  unmount();

  const none = render(SidebarIcons, { ...strip, filters: [] });

  expect([withFilters, none.container.querySelectorAll(".rule").length]).toEqual([2, 1]);
});
