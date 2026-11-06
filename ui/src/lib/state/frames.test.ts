import { describe, expect, it } from "vitest";
import {
  droppedFrames,
  type Frame,
  frameInterval,
  frameStats,
  nextFrameLimit,
  percentile,
  recent,
} from "./frames";

/** Frames of the given durations, one after another from time 0. */
function framesOf(durations: number[]): Frame[] {
  let end = 0;
  return durations.map((duration) => {
    end += duration;
    return { end, duration };
  });
}

describe("percentile", () => {
  it("is the value at or below which that share of the values fall", () => {
    const values = Array.from({ length: 100 }, (_, i) => 100 - i);
    expect(percentile(values, 0.95)).toBe(95);
    expect(percentile(values, 0.5)).toBe(50);
  });

  it("is the one value when there is one", () => {
    expect(percentile([7], 0.95)).toBe(7);
  });

  it("is nothing without values", () => {
    expect(percentile([], 0.95)).toBeUndefined();
  });

  it("leaves the values in their order", () => {
    const values = [3, 1, 2];
    percentile(values, 0.5);
    expect(values).toEqual([3, 1, 2]);
  });
});

describe("frameStats", () => {
  it("says the latest, the p95, the worst and how many went over the limit", () => {
    const durations = [...Array.from({ length: 18 }, () => 16), 30, 40, 17];
    expect(frameStats(framesOf(durations), 25)).toEqual({
      count: 21,
      latest: 17,
      p95: 30,
      worst: 40,
      overLimit: 2,
    });
  });

  it("counts a frame exactly at the limit as within it", () => {
    expect(frameStats(framesOf([25, 25.5]), 25)?.overLimit).toBe(1);
  });

  it("is nothing without frames", () => {
    expect(frameStats([], 25)).toBeUndefined();
  });
});

describe("recent", () => {
  it("keeps the frames that ended within the span before now", () => {
    const frames = framesOf([16, 16, 16, 16]);
    expect(recent(frames, 64, 40).map((frame) => frame.end)).toEqual([32, 48, 64]);
  });
});

describe("frameInterval", () => {
  it("is the median frame", () => {
    expect(frameInterval(framesOf([7, 7, 7, 50, 7]))).toBe(7);
  });

  it("is a 60 Hz frame before there are frames", () => {
    expect(frameInterval([])).toBeCloseTo(16.67, 2);
  });
});

describe("nextFrameLimit", () => {
  it("allows a frame and a half, so a key that misses its frame is over it", () => {
    expect(nextFrameLimit(1000 / 60)).toBeCloseTo(25, 5);
    expect(nextFrameLimit(1000 / 144)).toBeCloseTo(10.42, 2);
  });
});

describe("droppedFrames", () => {
  it("counts the frames a long one took the place of", () => {
    const interval = 1000 / 60;
    expect(droppedFrames(16.7, interval)).toBe(0);
    expect(droppedFrames(19, interval)).toBe(0);
    expect(droppedFrames(33.4, interval)).toBe(1);
    expect(droppedFrames(100, interval)).toBe(5);
  });

  it("counts none for a frame shorter than the interval", () => {
    expect(droppedFrames(4, 1000 / 60)).toBe(0);
  });
});
