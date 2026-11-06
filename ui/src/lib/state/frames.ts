// The frame statistics the performance HUD shows against ADR 0017's budgets. Times are milliseconds on the
// clock of `performance.now()`.

/** A frame the window drew: when it ended, and how long after the one before it. */
export interface Frame {
  end: number;
  duration: number;
}

export interface FrameStats {
  count: number;
  latest: number;
  p95: number;
  worst: number;
  overLimit: number;
}

/** The frame a display at 60 Hz draws, for a window that has drawn none yet. */
const SIXTY_HZ = 1000 / 60;

/**
 * The value at or below which the share `p` of `values` falls, by the nearest rank: of 100 values the p95 is
 * the 95th smallest.
 */
export function percentile(values: readonly number[], p: number): number | undefined {
  const sorted = values.toSorted((a, b) => a - b);
  return sorted[Math.max(Math.ceil(p * sorted.length) - 1, 0)];
}

export function frameStats(frames: readonly Frame[], limit: number): FrameStats | undefined {
  const durations = frames.map((frame) => frame.duration);
  const latest = durations.at(-1);
  const p95 = percentile(durations, 0.95);
  if (latest === undefined || p95 === undefined) return undefined;
  return {
    count: durations.length,
    latest,
    p95,
    // A long recording holds more frames than a call can take as arguments, so no `Math.max(...durations)`.
    worst: durations.reduce((worst, duration) => Math.max(worst, duration)),
    overLimit: durations.filter((duration) => duration > limit).length,
  };
}

/** The frames that ended in the last `span` before `now`. */
export function recent(frames: readonly Frame[], now: number, span: number): Frame[] {
  return frames.filter((frame) => frame.end > now - span);
}

/** The display's frame time, from the median of `frames`. */
export function frameInterval(frames: readonly Frame[]): number {
  return (
    percentile(
      frames.map((frame) => frame.duration),
      0.5,
    ) ?? SIXTY_HZ
  );
}

/**
 * The longest a key can take to reach the screen and still make the next frame. A key arrives anywhere
 * within a frame, so the next one can come up to a whole frame later; half a frame more leaves room for the
 * time the OS takes to hand the key over.
 */
export function nextFrameLimit(interval: number): number {
  return interval * 1.5;
}

/** The frames that a frame of `duration` took the place of: a frame twice the interval dropped one. */
export function droppedFrames(duration: number, interval: number): number {
  return Math.max(Math.round(duration / interval) - 1, 0);
}
