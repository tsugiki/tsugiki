import { render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Opened } from "$lib/state/home.svelte";
import HomePage from "./HomePage.svelte";

const NOW = new Date("2026-11-23T12:00:00Z");

const ROWS: Opened[] = [{ name: "python/cpython", opened_at: "2026-11-23T11:59:30Z" }];

/** The Home page, whose row says when the user opened its Repository, which the clock moves on. */
const shown = () =>
  render(HomePage, {
    rows: ROWS,
    keysActive: true,
    onopen: vi.fn(),
    onback: vi.fn(),
    onadd: vi.fn(),
    onsettings: vi.fn(),
  });

/** The words of the row, which the clock moves on. */
const words = () => screen.getByRole("button", { name: /opened/ }).textContent;

beforeEach(() => {
  vi.useFakeTimers({ now: NOW, toFake: ["Date", "setInterval", "clearInterval"] });
});

afterEach(() => {
  vi.useRealTimers();
});

it("follows the system's time, a minute at a time", async () => {
  shown();
  expect(words()).toContain("opened just now");

  await vi.advanceTimersByTimeAsync(60_000);

  expect(words()).toContain("opened 1 min ago");
});

it("says a time a day old as a day old (#1066)", async () => {
  shown();

  await vi.advanceTimersByTimeAsync(24 * 60 * 60_000);

  expect(words()).toContain("opened 1 d ago");
});

it("reads the system's time again when the window comes back to the front (#1066)", async () => {
  shown();
  // The clock starts its effect after the page is drawn.
  await vi.advanceTimersByTimeAsync(0);
  // The window was hidden, and its timers stopped, so the system's time moved on without the clock.
  vi.setSystemTime(new Date("2026-11-23T14:00:00Z"));

  document.dispatchEvent(new Event("visibilitychange"));
  await vi.advanceTimersByTimeAsync(0);

  expect(words()).toContain("opened 2 h ago");
});

it("stops its clock when it goes", () => {
  const { unmount } = shown();

  unmount();

  expect(vi.getTimerCount()).toBe(0);
});
