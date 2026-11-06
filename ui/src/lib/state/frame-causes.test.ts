import { describe, expect, it } from "vitest";
import { causeOf, describeCause, type Happening } from "./frame-causes";

const happenings: Happening[] = [
  { at: 100, cause: { kind: "key" } },
  { at: 940, cause: { kind: "click" } },
  { at: 990, cause: { kind: "view_query" } },
];

describe("causeOf", () => {
  it("is the last thing that happened while the frame took its time", () => {
    expect(causeOf({ end: 1000, duration: 40 }, happenings)).toEqual({ kind: "view_query" });
  });

  it("counts what happened just before the frame began, since a key's time comes from the OS", () => {
    expect(causeOf({ end: 960, duration: 16 }, happenings)).toEqual({ kind: "click" });
    expect(causeOf({ end: 1000, duration: 40 }, happenings.slice(0, 2))).toEqual({ kind: "click" });
  });

  it("is nothing when the last thing happened long before the frame", () => {
    expect(causeOf({ end: 500, duration: 30 }, happenings)).toBeUndefined();
  });

  it("leaves out what happened after the frame ended", () => {
    expect(causeOf({ end: 950, duration: 30 }, happenings)).toEqual({ kind: "click" });
  });

  it("is nothing when nothing happened", () => {
    expect(causeOf({ end: 1000, duration: 40 }, [])).toBeUndefined();
  });
});

describe("describeCause", () => {
  it("says each cause in words, and never which key", () => {
    expect(describeCause({ kind: "key" })).toBe("after a key");
    expect(describeCause({ kind: "click" })).toBe("after a click");
    expect(describeCause({ kind: "view_query" })).toBe("after a view's answer");
    expect(describeCause({ kind: "issue_text", number: 1234 })).toBe(
      "after #1234's text arrived from GitHub",
    );
    expect(describeCause(undefined)).toBe("with no known cause");
  });
});
