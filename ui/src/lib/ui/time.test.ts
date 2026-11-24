import { describe, expect, it } from "vitest";
import { stampOf } from "./time";

describe("stampOf", () => {
  const AT = "2026-11-04T10:00:00Z";

  it('writes a date and a time, as a tooltip beside an "… ago" gives them', () => {
    expect(stampOf(AT)).toBe(new Date(AT).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }));
  });
});
