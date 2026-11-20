import { describe, expect, it } from "vitest";
import { HOUR_POINTS } from "$lib/testing/fixtures";
import { pointsTip, pointsWords } from "./points";

const points = HOUR_POINTS;

const time = (at: string) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

describe("pointsWords", () => {
  it("says the points left and when they reset", () => {
    expect(pointsWords(points)).toBe(`1,240 points left · resets ${time(points.resets_at)}`);
  });
});

describe("pointsTip", () => {
  it("says what Tsugiki spent of the hour's points, and that other tools spent the rest", () => {
    expect(pointsTip(points)).toBe(
      "This hour: 3,760 of 5,000 points used. Tsugiki spent 3,610 of them; other tools using your token spent the rest.",
    );
  });

  it("says when Tsugiki spent them all", () => {
    expect(pointsTip({ ...points, used: 410, remaining: 4590, spent: 410 })).toBe(
      "This hour: 410 of 5,000 points used. Tsugiki spent all of them.",
    );
  });

  it("says since when Tsugiki counted, after a restart in the middle of the hour", () => {
    const since = "2026-11-17T14:05:00Z";
    expect(pointsTip({ ...points, spent: 410, since })).toBe(
      `This hour: 3,760 of 5,000 points used. Tsugiki spent 410 since ${time(since)}.`,
    );
  });
});
