import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { ActionNotice, NOTICE_MS } from "$lib/state/action-notice.svelte";
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

afterEach(() => {
  vi.useRealTimers();
});

it("stays in the page empty as a polite live region, and holds no focusable element", () => {
  const { hud, notice } = harness();
  // The bar shows unless a caller says otherwise, as every screen but a narrow one does.
  render(ActionPanel, { notice, hud });

  const region = panel();
  expect(region.getAttribute("aria-live")).toBe("polite");
  expect(region.textContent).toBe("");
  expect(region.querySelectorAll("a, button, input, select, textarea, [tabindex]")).toHaveLength(0);

  region.focus();
  expect(document.activeElement).not.toBe(region);
});

it("says what an action did, and goes five seconds after it shows", async () => {
  vi.useFakeTimers();
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud, statusBarVisible: true });

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
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud, statusBarVisible: true });
  notice.show("PR #162138 muted");
  await tick();

  await fireEvent.mouseEnter(panel());
  vi.advanceTimersByTime(NOTICE_MS * 2);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");

  await fireEvent.mouseLeave(panel());
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("sits above the status bar, at the edge while the bar is hidden, and above the HUD while it shows", async () => {
  const { hud, notice } = harness();
  const { rerender } = render(ActionPanel, { notice, hud, statusBarVisible: true });
  expect(panel().style.bottom).toBe("32px");

  await rerender({ notice, hud, statusBarVisible: false });
  expect(panel().style.bottom).toBe("0px");

  hud.visible = true;
  hud.height = 100;
  await tick();
  expect(panel().style.bottom).toBe("140px");
});
