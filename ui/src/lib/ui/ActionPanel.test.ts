import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, expect, it, onTestFinished, vi } from "vitest";
import { ActionNotice, NOTICE_MS } from "$lib/state/action-notice.svelte";
import { HUD_BOTTOM } from "$lib/state/hud.svelte";
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
  const { hud, notice } = harness();
  render(ActionPanel, { notice, hud });
  notice.show("PR #162138 muted");
  await tick();

  await fireEvent(panel(), new Event("pointerenter"));
  vi.advanceTimersByTime(NOTICE_MS * 2);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");

  await fireEvent(panel(), new Event("pointerleave"));
  vi.advanceTimersByTime(NOTICE_MS - 1);
  await tick();
  expect(panel().textContent).toBe("PR #162138 muted");
  vi.advanceTimersByTime(1);
  await tick();
  expect(panel().textContent).toBe("");
});

it("says how tall it draws, so a list that fills the window can leave it the room", () => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(28);
  onTestFinished(() => {
    vi.restoreAllMocks();
  });
  const { hud, notice } = harness();
  const onheight = vi.fn();
  render(ActionPanel, { notice, hud, onheight });
  expect(onheight).toHaveBeenLastCalledWith(28);
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
  expect(panel().style.bottom).toBe(`${String(HUD_BOTTOM + 100 + 8)}px`);
});
