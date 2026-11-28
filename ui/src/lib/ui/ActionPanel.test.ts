import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { ActionNotice, NOTICE_MS, PANEL_EDGE, PANEL_GAP } from "$lib/state/action-notice.svelte";
import { measureHeight } from "$lib/testing/helpers";
import { testHud } from "$lib/testing/state";
import ActionPanel from "./ActionPanel.svelte";

function harness() {
  const core = {
    timeRunning: () => Promise.resolve(null),
    recordMeasurement: () => Promise.resolve("a line"),
    recordMiss: () => Promise.resolve("a line"),
  };
  return { hud: testHud(core), notice: new ActionNotice() };
}

const panel = () => screen.getByRole("log", { name: "What the action did" });

/** A point on the panel as jsdom draws it: 28 px tall in the window's bottom-right corner. */
const onPanel = { x: window.innerWidth - PANEL_EDGE - 10, y: window.innerHeight - 10 };

const moveTo = (x: number, y: number) =>
  fireEvent(window, new MouseEvent("pointermove", { clientX: x, clientY: y }));

afterEach(() => {
  vi.useRealTimers();
});

it("stays in the page empty as a polite live region, and holds no focusable element", async () => {
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });

  const region = panel();
  expect(region.getAttribute("aria-live")).toBe("polite");
  expect(region.textContent).toBe("");
  expect(region.style.pointerEvents).toBe("none");
  expect(region.querySelectorAll("a, button, input, select, textarea, [tabindex]")).toHaveLength(0);

  // A pointer on the empty panel's place holds nothing.
  await moveTo(onPanel.x, onPanel.y);

  region.focus();
  expect(document.activeElement).not.toBe(region);
});

it("says what an action did, and goes five seconds after it shows", async () => {
  vi.useFakeTimers();
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });

  notice.show("Issue #141148 archived");
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");

  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("holds the countdown while the pointer is on the panel, and resumes it when it leaves", async () => {
  vi.useFakeTimers();
  measureHeight(28, { width: 120 });
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });
  notice.show("PR #162138 muted");
  await tick();
  expect(panel().style.pointerEvents).toBe("none");

  await moveTo(onPanel.x, onPanel.y);
  vi.advanceTimersByTime(NOTICE_MS * 2);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");

  // Moving within the box keeps it held.
  await moveTo(onPanel.x - 10, onPanel.y - 5);
  await moveTo(100, onPanel.y);
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("lets the countdown go when the status bar raises the panel from under a still pointer", async () => {
  vi.useFakeTimers();
  measureHeight(28, { width: 120 });
  const { hud, notice } = harness();
  const { rerender } = render(ActionPanel, { notice, hud, statusBarHeight: 0 });
  notice.show("Issue #141148 archived");
  await tick();
  await moveTo(onPanel.x, onPanel.y);
  vi.advanceTimersByTime(2000);

  await rerender({ notice, hud, statusBarHeight: 24 });
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("lets the countdown go when the window shrinks the panel away from a still pointer", async () => {
  vi.useFakeTimers();
  measureHeight(28, { width: 120 });
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });
  notice.show("Issue #141148 archived");
  await tick();
  await moveTo(onPanel.x, onPanel.y);
  vi.advanceTimersByTime(2000);

  vi.spyOn(window, "innerHeight", "get").mockReturnValue(600);
  await fireEvent(window, new Event("resize"));
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("resumes the countdown when the pointer leaves the window, or the window loses focus", async () => {
  vi.useFakeTimers();
  measureHeight(28, { width: 120 });
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });
  notice.show("Issue #141148 archived");
  await tick();
  await moveTo(onPanel.x, onPanel.y);

  // A pointerout to another element in the window leaves the countdown held.
  await fireEvent(window, new MouseEvent("pointerout", { relatedTarget: document.body }));
  vi.advanceTimersByTime(NOTICE_MS);
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");

  // Nowhere to go is the pointer leaving the window.
  await fireEvent(window, new MouseEvent("pointerout", { relatedTarget: null }));
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("Issue #141148 archived");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");

  notice.show("PR #162138 muted");
  await tick();
  await moveTo(onPanel.x, onPanel.y);
  await fireEvent(window, new Event("blur"));
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("says the room a list that fills the window must leave for it, above the bar or the HUD", async () => {
  measureHeight(28);
  const { hud, notice } = harness();
  const onclearance = vi.fn();
  render(ActionPanel, { notice, hud, statusBarHeight: 24, onclearance });
  expect(onclearance).toHaveBeenLastCalledWith(PANEL_GAP + 28);

  hud.visible = true;
  hud.height = 100;
  await tick();
  expect(onclearance).toHaveBeenLastCalledWith(8 + 100 + PANEL_GAP + 28);
});

it("sits above the status bar at the height it draws, at the edge while it draws nothing, and above the HUD", async () => {
  const { hud, notice } = harness();
  const { rerender } = render(ActionPanel, { notice, hud, statusBarHeight: 0 });
  expect(panel().style.bottom).toBe("0px");

  await rerender({ notice, hud, statusBarHeight: 24 });
  expect(panel().style.bottom).toBe("32px");

  hud.visible = true;
  hud.height = 100;
  await tick();
  expect(panel().style.bottom).toBe("140px");

  await rerender({ notice, hud, statusBarHeight: 43 });
  expect(panel().style.bottom).toBe("159px");
});
