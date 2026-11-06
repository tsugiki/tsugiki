import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import { HudState } from "$lib/state/hud.svelte";
import PerformanceHud from "./PerformanceHud.svelte";

function shownHud() {
  const clock = { now: () => 0, requestFrame: () => 1, cancelFrame: () => {}, afterPaint: () => {} };
  const core = {
    timeRunning: () => Promise.resolve(null),
    recordMeasurement: vi.fn(() => Promise.resolve("Performance of Tsugiki 0.1.0 over 1.0 s")),
    recordMiss: () => Promise.resolve("a miss"),
  };
  const hud = new HudState(clock, core);
  hud.toggle();
  return { hud, core };
}

/** The value and the budget on the line named `name`, and whether it shows as a miss. */
function line(name: string) {
  const row = screen.getByRole("row", { name: new RegExp(`^${name}`) });
  const [value, budget] = within(row).getAllByRole("cell");
  return { value: value?.textContent, budget: budget?.textContent, missed: row.classList.contains("miss") };
}

it("shows a dash for what it hasn't measured yet", () => {
  const { hud } = shownHud();
  render(PerformanceHud, { hud, refreshing: false });
  expect(line("Start-up")).toEqual({ value: "—", budget: "1 s", missed: false });
  expect(line("View query")).toEqual({ value: "—", budget: "50 ms", missed: false });
});

it("shows each number against its budget, and marks a miss", () => {
  const { hud } = shownHud();
  hud.startup = 1240;
  hud.key = 12;
  hud.viewQuery = 31.25;
  hud.lastRefresh = 3;
  hud.frames = { count: 600, latest: 16.6, p95: 18, worst: 41, overLimit: 2 };
  render(PerformanceHud, { hud, refreshing: false });

  expect(line("Start-up")).toEqual({ value: "1240 ms", budget: "1 s", missed: true });
  expect(line("Key to frame")).toEqual({ value: "12.0 ms", budget: "25.0 ms", missed: false });
  expect(line("Frame, latest")).toEqual({ value: "16.6 ms", budget: "25 ms", missed: false });
  expect(line("Frame, p95")).toEqual({ value: "18.0 ms", budget: "25 ms", missed: false });
  expect(line("Frame, worst")).toEqual({ value: "41.0 ms", budget: "25 ms", missed: true });
  expect(line("Frames over 25 ms")).toEqual({ value: "2 of 600", budget: "none", missed: true });
  expect(line("Dropped in last refresh")).toEqual({ value: "3", budget: "none", missed: true });
  expect(line("View query")).toEqual({ value: "31.3 ms", budget: "50 ms", missed: false });
});

it("tells the HUD when a background refresh starts and ends", async () => {
  const { hud } = shownHud();
  const refreshing = vi.spyOn(hud, "refreshing");
  const { rerender } = render(PerformanceHud, { hud, refreshing: true });
  expect(refreshing).toHaveBeenLastCalledWith(true);
  await rerender({ hud, refreshing: false });
  expect(refreshing).toHaveBeenLastCalledWith(false);
});

it("records, and shows the line it wrote to the log", async () => {
  const { hud, core } = shownHud();
  render(PerformanceHud, { hud, refreshing: false });
  await fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
  await fireEvent.click(screen.getByRole("button", { name: "Stop recording" }));
  expect(core.recordMeasurement).toHaveBeenCalledOnce();
  expect((await screen.findByText(/^Written to tsugiki\.log/)).textContent).toBe(
    "Written to tsugiki.log: Performance of Tsugiki 0.1.0 over 1.0 s",
  );
});

it("says what went wrong", () => {
  const { hud } = shownHud();
  hud.error = "Error: no log";
  render(PerformanceHud, { hud, refreshing: false });
  expect(screen.getByRole("alert").textContent).toBe("Error: no log");
});

it("shows the last Issue opened, from the database and from GitHub", async () => {
  const { hud } = shownHud();
  hud.issue = { stage: "github", number: 1234, fromDatabase: 240 };
  render(PerformanceHud, { hud, refreshing: false });
  expect(line("Issue from the database")).toEqual({ value: "240.0 ms", budget: "200 ms", missed: true });
  expect(line("Issue from GitHub")).toEqual({ value: "waiting", budget: "none set", missed: false });
  hud.issue = { stage: "read", number: 1234, fromDatabase: 12, fromGitHub: 640 };
  await Promise.resolve();
  expect(line("Issue from the database")).toEqual({ value: "12.0 ms", budget: "200 ms", missed: false });
  expect(line("Issue from GitHub").value).toBe("640.0 ms");
});

it("says when the database held the Issue's text, and before one opens", async () => {
  const { hud } = shownHud();
  render(PerformanceHud, { hud, refreshing: false });
  expect(line("Issue from the database").value).toBe("—");
  expect(line("Issue from GitHub").value).toBe("—");
  hud.issue = { stage: "database", number: 1 };
  await Promise.resolve();
  expect(line("Issue from the database").value).toBe("waiting");
  hud.issue = { stage: "held", number: 1, fromDatabase: 5 };
  await Promise.resolve();
  expect(line("Issue from GitHub").value).toBe("held");
});

it("shows the last long frame, how long ago it was, and its cause", async () => {
  const { hud } = shownHud();
  render(PerformanceHud, { hud, refreshing: false });
  expect(screen.queryByText(/^Last long frame/)).toBeNull();
  hud.longFrame = { end: 1000, duration: 40.1, cause: { kind: "click" } };
  hud.now = 4400;
  await Promise.resolve();
  expect(screen.getByText(/^Last long frame/).textContent).toBe(
    "Last long frame: 40.1 ms, 3 s ago, after a click",
  );
});
