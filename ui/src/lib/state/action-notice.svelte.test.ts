import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ActionNotice,
  authorWords,
  didWords,
  itemName,
  NOTICE_MS,
  overPanel,
  overRect,
} from "./action-notice.svelte";

afterEach(() => {
  vi.useRealTimers();
});

describe("what the panel says", () => {
  it("names an Issue and a PR by kind and number", () => {
    expect(itemName({ kind: "issue", number: 141148 })).toBe("Issue #141148");
    expect(itemName({ kind: "pull_request", number: 162138 })).toBe("PR #162138");
  });

  it("says in the past tense what the action did", () => {
    expect(didWords({ kind: "issue", number: 141148 }, "archived")).toBe("Issue #141148 archived");
    expect(didWords({ kind: "pull_request", number: 162138 }, "muted")).toBe("PR #162138 muted");
  });

  it("names the author Mute author acts on", () => {
    expect(authorWords("some-author", "muted")).toBe("Muted some-author");
    expect(authorWords("some-author", "unmuted")).toBe("Unmuted some-author");
  });
});

describe("the countdown", () => {
  it("says what an action did, and goes five seconds after it shows", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();
    expect(notice.text).toBeUndefined();

    notice.show("Issue #141148 archived");
    expect(notice.text).toBe("Issue #141148 archived");

    vi.advanceTimersByTime(NOTICE_MS - 1);
    expect(notice.text).toBe("Issue #141148 archived");
    vi.advanceTimersByTime(1);
    expect(notice.text).toBeUndefined();
  });

  it("replaces the words and starts the five seconds again", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();

    notice.show("Issue #141148 archived");
    vi.advanceTimersByTime(4000);
    notice.show("PR #162138 muted");
    expect(notice.text).toBe("PR #162138 muted");

    vi.advanceTimersByTime(4000);
    expect(notice.text).toBe("PR #162138 muted");
    vi.advanceTimersByTime(1000);
    expect(notice.text).toBeUndefined();
  });

  it("holds the countdown under the pointer, and resumes it with the time it had left", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();
    notice.show("Issue #141148 archived");
    vi.advanceTimersByTime(3000);

    notice.hold();
    vi.advanceTimersByTime(10_000);
    expect(notice.text).toBe("Issue #141148 archived");

    notice.release();
    vi.advanceTimersByTime(1999);
    expect(notice.text).toBe("Issue #141148 archived");
    vi.advanceTimersByTime(1);
    expect(notice.text).toBeUndefined();
  });

  it("holds new words shown while the pointer is on the panel", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();
    notice.show("Issue #141148 archived");
    notice.hold();

    notice.show("PR #162138 muted");
    vi.advanceTimersByTime(NOTICE_MS);
    expect(notice.text).toBe("PR #162138 muted");

    notice.release();
    vi.advanceTimersByTime(NOTICE_MS - 1);
    expect(notice.text).toBe("PR #162138 muted");
    vi.advanceTimersByTime(1);
    expect(notice.text).toBeUndefined();
  });

  it("takes the panel away when the Repository changes", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();
    notice.show("Issue #141148 archived");

    notice.clear();
    expect(notice.text).toBeUndefined();
    vi.advanceTimersByTime(NOTICE_MS);
    expect(notice.text).toBeUndefined();
  });

  it("ignores a hold and a release when there is nothing to hold", () => {
    vi.useFakeTimers();
    const notice = new ActionNotice();
    notice.hold();
    notice.release();
    notice.clear();
    expect(notice.text).toBeUndefined();
  });
});

describe("where the pointer is", () => {
  const rect = { left: 10, right: 20, top: 10, bottom: 20 };

  it("reads a point inside the box, and outside it on each side", () => {
    expect(overRect(rect, 15, 15)).toBe(true);
    expect(overRect(rect, 5, 15)).toBe(false);
    expect(overRect(rect, 25, 15)).toBe(false);
    expect(overRect(rect, 15, 5)).toBe(false);
    expect(overRect(rect, 15, 25)).toBe(false);
  });

  it("is over no panel before the panel is in the page, and over it by its box after", () => {
    expect(overPanel(undefined, { clientX: 15, clientY: 15 })).toBe(false);

    const panel = document.createElement("aside");
    panel.getBoundingClientRect = () => ({
      ...rect,
      width: 10,
      height: 10,
      x: 10,
      y: 10,
      toJSON: () => ({}),
    });
    expect(overPanel(panel, { clientX: 15, clientY: 15 })).toBe(true);
  });
});
