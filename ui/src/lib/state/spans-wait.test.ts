import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SPANS_WAIT, waitForSpans } from "./spans-wait";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/** A promise of spans, and the way to send them. */
function later() {
  let send: (spans: string) => void = () => {};
  const spans = new Promise<string | undefined>((resolve) => (send = resolve));
  return { spans, send };
}

it("gives the spans that come within the wait", async () => {
  const { spans, send } = later();
  const waited = waitForSpans(spans);

  send("spans");

  expect(await waited()).toBe("spans");
});

it("gives nothing once the wait has passed, and the rows show plain", async () => {
  const { spans, send } = later();
  const waited = waitForSpans(spans);
  const early = waited();

  await vi.advanceTimersByTimeAsync(SPANS_WAIT);
  send("late");

  expect(await early).toBeUndefined();
  expect(await spans).toBe("late");
});

it("gives the spans that came before rows that were slower than the wait", async () => {
  const { spans, send } = later();
  const waited = waitForSpans(spans);

  send("spans");
  await vi.advanceTimersByTimeAsync(SPANS_WAIT);

  expect(await waited()).toBe("spans");
});
