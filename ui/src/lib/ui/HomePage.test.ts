import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Opened } from "$lib/state/home.svelte";
import HomePage from "./HomePage.svelte";

const NOW = new Date("2026-11-23T12:00:00Z");

const ROWS: Opened[] = [
  { name: "python/cpython", opened_at: "2026-11-23T11:59:30Z" },
  { name: "python/peps", opened_at: "2026-11-23T10:00:00Z" },
  { name: "a/never", opened_at: null },
];

beforeEach(() => {
  vi.useFakeTimers({ now: NOW, toFake: ["Date", "setInterval", "clearInterval"] });
});

afterEach(() => {
  vi.useRealTimers();
});

const shown = (keysActive = true) => {
  const props = { onopen: vi.fn(), onback: vi.fn(), onadd: vi.fn(), onsettings: vi.fn() };
  const { rerender } = render(HomePage, { rows: ROWS, keysActive, ...props });
  return { ...props, rerender };
};

const rows = () => within(screen.getByRole("list", { name: "Repositories" })).getAllByRole("button");
const current = () => rows().find((row) => row.getAttribute("aria-current") === "true");

it("lists each Repository with when the user opened it, and none for one never opened here", () => {
  shown();
  expect(rows().map((row) => row.textContent.replace(/\s+/g, " ").trim())).toEqual([
    "python/cpython opened just now",
    "python/peps opened 2 h ago",
    "a/never",
  ]);
});

it("says when the user opened a Repository in its tooltip, with the date and the time (#1066)", () => {
  shown();
  const opened = screen.getAllByTitle(/^Opened /);
  expect(opened.map((one) => one.getAttribute("title"))).toEqual([
    `Opened ${new Date("2026-11-23T11:59:30Z").toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`,
    `Opened ${new Date("2026-11-23T10:00:00Z").toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`,
  ]);
});

it("moves the time on with the clock, so a time a day old says so (#1066)", async () => {
  shown();
  expect(rows()[0]?.textContent).toContain("opened just now");
  await vi.advanceTimersByTimeAsync(24 * 60 * 60_000);
  expect(rows()[0]?.textContent).toContain("opened 1 d ago");
  expect(rows()[1]?.textContent).toContain("opened 1 d ago");
});

it("starts on the top row with the focus, and keeps the cursor between the first row and the last", async () => {
  shown();
  expect(document.activeElement).toBe(rows()[0]);
  expect(current()).toBe(rows()[0]);

  await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowUp" });
  expect(current()).toBe(rows()[0]);
  for (let press = 0; press < 4; press += 1) {
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
  }
  expect(current()).toBe(rows()[2]);
  expect(document.activeElement).toBe(rows()[2]);
});

it("opens the row under the cursor on Enter with nothing focused, but not on a held Enter", async () => {
  const { onopen } = shown();
  await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
  (document.activeElement as HTMLElement).blur();

  await fireEvent.keyDown(document.body, { key: "Enter", repeat: true });
  expect(onopen).not.toHaveBeenCalled();
  await fireEvent.keyDown(document.body, { key: "Enter" });
  expect(onopen).toHaveBeenCalledWith("python/peps");
});

it("opens a focused row once on a held Enter, whose repeats the browser would click again", async () => {
  shown();
  const row = rows()[0] as Element;
  // A press dispatched with its default left alone is one the browser clicks the button on.
  expect(await fireEvent.keyDown(row, { key: "Enter" })).toBe(true);
  expect(await fireEvent.keyDown(row, { key: "Enter", repeat: true })).toBe(false);
});

it("keeps the cursor on its row as the rows change, and where a row that leaves was", async () => {
  const { rerender } = shown();
  rows()[2]?.focus();
  await rerender({ rows: [ROWS[0], ROWS[2]] as Opened[] });
  expect(current()?.textContent).toContain("a/never");

  await rerender({ rows: ROWS });
  rows()[1]?.focus();
  await rerender({ rows: [ROWS[0], ROWS[2]] as Opened[] });
  expect(current()?.textContent).toContain("a/never");

  rows()[1]?.focus();
  await rerender({ rows: [ROWS[0]] as Opened[] });
  expect(current()?.textContent).toContain("python/cpython");
});

it("leaves the cursor where a row last stood when it moves and then leaves", async () => {
  const [cpython, peps, never] = ROWS as [Opened, Opened, Opened];
  const last: Opened = { name: "a/last", opened_at: null };
  const { rerender } = shown();
  await rerender({ rows: [cpython, peps, never, last] });
  rows()[3]?.focus();
  await rerender({ rows: [cpython, last, peps, never] });
  expect(current()?.textContent).toContain("a/last");

  await rerender({ rows: [cpython, peps, never] });
  expect(current()?.textContent).toContain("python/peps");
});

it("moves the cursor to a row that takes the focus, and opens a row on a click", async () => {
  const { onopen } = shown();
  rows()[2]?.focus();
  await tick();
  expect(current()).toBe(rows()[2]);
  await fireEvent.click(rows()[1] as Element);
  expect(onopen).toHaveBeenCalledWith("python/peps");
});

it("goes back on Esc", async () => {
  const { onback } = shown();
  await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
  expect(onback).toHaveBeenCalledOnce();
});

it("leaves its keys to a dialog on top", async () => {
  const { onback, onopen } = shown(false);
  await fireEvent.keyDown(document.body, { key: "Escape" });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  await fireEvent.keyDown(document.body, { key: "Enter" });
  expect(onback).not.toHaveBeenCalled();
  expect(onopen).not.toHaveBeenCalled();
  expect(current()).toBe(rows()[0]);
});

it("opens Add repository and Settings from their buttons", async () => {
  const { onadd, onsettings } = shown();
  await fireEvent.click(screen.getByRole("button", { name: "Add repository…" }));
  await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(onadd).toHaveBeenCalledOnce();
  expect(onsettings).toHaveBeenCalledOnce();
});
