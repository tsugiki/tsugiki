// The performance HUD (ADR 0017): what the window measures of itself against the stage 1 budgets, so the
// maintainer can measure a release build before tagging it. It starts hidden each run, and while hidden it
// runs no frame loop and hears no keys. It still times the start-up, view queries, the Issues the user opens
// and the jumps in a diff then, at the cost of reading the clock and of a frame for each part of an Issue or
// a jump, and logs the times that miss their budgets (#151).
import {
  commands,
  type IssueOpenings_Deserialize as IssueOpenings,
  type Jumps,
  type Measurement_Deserialize as Measurement,
} from "$lib/data/bindings";
import { type Cause, causeOf, type Happening } from "./frame-causes";
import { droppedFrames, type Frame, frameInterval, frameStats, type FrameStats, recent } from "./frames";
import { type MissCore, MissLog, type ObserverClass } from "./misses";

/**
 * ADR 0017's budgets, in milliseconds. A key's budget is the next frame, which `nextFrameLimit` sets, and an
 * Issue's text read from GitHub has none, since it waits on the network.
 */
export const BUDGETS = {
  startup: 1000,
  frame: 25,
  viewQuery: 50,
  issueFromDatabase: 200,
  jump: 100,
} as const;

/** The time an Issue's text from GitHub can take before the log mentions it. No budget says this. */
const SLOW_FROM_GITHUB = 2000;

/** The time a happening can still be a long frame's cause, so the HUD keeps no more than this of them. */
const CAUSES_KEPT = 1000;

/** A frame over the limit, and what came before it. */
export interface LongFrame extends Frame {
  cause: Cause | undefined;
}

/**
 * The last Issue opened while the HUD showed, timed from the click: until the window showed what the
 * database held, and until its text arrived from GitHub if the database didn't hold it.
 */
export type IssueOpening =
  | { stage: "database"; number: number }
  | { stage: "github"; number: number; fromDatabase: number }
  | { stage: "held"; number: number; fromDatabase: number }
  | { stage: "read"; number: number; fromDatabase: number; fromGitHub: number };

/**
 * A jump in a PR's diff: a scroll that shows rows the window doesn't hold. The HUD times it from the scroll
 * until its rows are painted, and until their spans are, if those come after.
 */
export interface Jump {
  /** The rows are on their way to the screen, highlighted if their spans came in time. */
  rowsShown(highlighted: boolean): void;
  /** The spans of rows that showed plain are on their way to the screen. */
  spansShown(): void;
}

/** Times jumps. The Files tab asks it for a `Jump` each time it shows rows it doesn't hold. */
export interface JumpTimer {
  jumped(): Jump;
}

/** The last jump's times: until its rows were painted, and until their spans were, or null if they haven't
 * been. */
export interface JumpTimes {
  rows: number;
  spans: number | null;
}

/** One Issue's times in a recording. `github` is null until its text arrives, and stays null when the
 * database held it. */
interface IssueTimes {
  database: number;
  github: number | null;
}

/**
 * An Issue being opened. `reading` waits for its text from GitHub, and `times` settles once the database's
 * part has been painted, which comes first.
 */
type Opening =
  | { stage: "clicked"; number: number; start: number }
  | { stage: "reading"; number: number; start: number; times: Promise<IssueTimes> }
  | { stage: "done"; number: number };

/** The span the HUD's frame numbers cover. */
const RECENT = 10_000;

/** The gap between changes to the HUD's numbers, so drawing them costs the window little. */
const SHOW_EVERY = 250;

export interface HudClock {
  now(): number;
  requestFrame(callback: (time: number) => void): number;
  cancelFrame(handle: number): void;
  /** Calls `callback` once the frame being drawn has been painted. */
  afterPaint(callback: () => void): void;
}

/** The core's commands the HUD calls. */
export interface HudCore extends MissCore {
  timeRunning(): Promise<number | null>;
  recordMeasurement(measurement: Measurement): Promise<string>;
}

/** The frames, keys, queries, refreshes and Issues a recording has seen since Start. */
interface Recording {
  started: number;
  frames: Frame[];
  longFrames: LongFrame[];
  keys: number[];
  viewQueries: number[];
  refreshes: number[];
  issues: IssueTimes[];
  jumps: { rows: number; plain: boolean }[];
}

/** The slowest of `times`, or nothing without any. */
function slowest(times: number[]) {
  return times.length === 0
    ? null
    : { count: times.length, worst_ms: times.reduce((worst, time) => Math.max(worst, time)) };
}

/** The Issues a recording opened, as the log has them. */
function issueTimes(issues: readonly IssueTimes[]): IssueOpenings | null {
  const fromDatabase = slowest(issues.map((issue) => issue.database));
  if (fromDatabase === null) return null;
  return {
    from_database: fromDatabase,
    from_github: slowest(issues.flatMap((issue) => (issue.github === null ? [] : [issue.github]))),
  };
}

/** The jumps a recording saw, as the log has them. */
function jumpTimes(jumps: readonly { rows: number; plain: boolean }[]): Jumps | null {
  const rows = slowest(jumps.map((jump) => jump.rows));
  return rows === null ? null : { rows, plain: jumps.filter((jump) => jump.plain).length };
}

export const browserClock: HudClock = {
  now: () => performance.now(),
  requestFrame: (callback) => requestAnimationFrame(callback),
  cancelFrame: (handle) => {
    cancelAnimationFrame(handle);
  },
  // A frame callback runs before the frame's style, layout and paint. A message posted from it arrives after.
  afterPaint: (callback) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => {
      callback();
    };
    channel.port2.postMessage(undefined);
  },
};

export class HudState implements JumpTimer {
  visible = $state(false);
  /** The last 10 s of frames, or nothing before the HUD has shown them. */
  frames = $state<FrameStats>();
  /** The display's frame, from the median of the last 10 s. */
  interval = $state(frameInterval([]));
  startup = $state<number>();
  /** The last key, to the frame after it. */
  key = $state<number>();
  /** The window's wait for its last view query. */
  viewQuery = $state<number>();
  /** The frames dropped during the last background refresh the HUD saw end. */
  lastRefresh = $state<number>();
  /** The last frame over the limit since the HUD showed. */
  longFrame = $state<LongFrame>();
  issue = $state<IssueOpening>();
  /** The last jump in a PR's diff. */
  jump = $state<JumpTimes>();
  /** The time of the HUD's last update, so a line can say how long ago something was. */
  now = $state(0);
  recording = $state(false);
  /** The line the last recording wrote to `tsugiki.log`. */
  written = $state<string>();
  error = $state<string>();
  #clock: HudClock;
  #core: HudCore;
  #recent: Frame[] = [];
  #previous: number | undefined;
  #shownAt = 0;
  /** The frame asked for, while the HUD shows. */
  #handle = 0;
  /** The one frame asked for while hidden, so a paint can be timed without a frame loop. */
  #paintHandle: number | undefined;
  /** The times the window has hidden, so a query can tell whether it hid while the query ran. */
  #hidings = 0;
  #misses: MissLog;
  #pendingKey: number | undefined;
  /** The callbacks waiting for the next frame to be painted, to read the clock then. */
  #afterNextPaint: ((at: number) => void)[] = [];
  #happenings: Happening[] = [];
  #opening: Opening | undefined;
  /** The last jump's times, so a jump before it can't change what shows. */
  #lastJump: JumpTimes | undefined;
  #refreshDropped: number | undefined;
  #recording: Recording | undefined;
  #startupSettled = false;
  /** The last answer `refreshing` heard since the HUD showed. A refresh it didn't see start isn't counted. */
  #lastHeard: boolean | undefined;

  constructor(clock: HudClock = browserClock, core: HudCore = commands) {
    this.#clock = clock;
    this.#core = core;
    this.#misses = new MissLog(clock, core);
    // Hidden or shown, the HUD times Issues, and a window that hides paints nothing until it shows again.
    document.addEventListener("visibilitychange", this.#onVisibility);
  }

  /** Logs the slow keys, clicks and long frames the browser reports, where it can. */
  watchBrowser(Observer: ObserverClass | undefined) {
    this.#misses.watch(Observer);
  }

  toggle() {
    if (this.visible) {
      this.#hide();
    } else {
      this.#show();
    }
  }

  /** Notes a key pressed at `at`, which the next frame times. The first key before a frame counts. */
  keyPressed(at: number) {
    if (!this.visible) return;
    this.#happened({ at, cause: { kind: "key" } });
    if (this.#pendingKey !== undefined) return;
    this.#pendingKey = at;
    const recording = this.#recording;
    this.#whenPainted((painted) => {
      this.#pendingKey = undefined;
      const took = painted - at;
      this.key = took;
      recording?.keys.push(took);
    });
  }

  /** Starts timing the opening of Issue `number`, which the user has clicked. */
  issueOpened(number: number) {
    this.#opening = { stage: "clicked", number, start: this.#clock.now() };
    this.issue = { stage: "database", number };
  }

  /** Says the window now shows Issue `number`, and whether that includes its text. */
  issueShown(number: number, hasText: boolean) {
    const opening = this.#opening;
    if (opening?.number !== number) return;
    if (opening.stage === "clicked") {
      const recording = this.#recording;
      // A recording holds the Issue from here, so one still waiting for its text counts.
      const times = new Promise<IssueTimes>((resolve) => {
        this.#whenPainted((painted) => {
          const fromDatabase = painted - opening.start;
          if (fromDatabase > BUDGETS.issueFromDatabase) {
            this.#misses.missed({ kind: "issue_from_database", number, ms: fromDatabase });
          }
          this.issue = { stage: hasText ? "held" : "github", number, fromDatabase };
          const shown = { database: fromDatabase, github: null };
          recording?.issues.push(shown);
          resolve(shown);
        });
      });
      this.#opening = hasText
        ? { stage: "done", number }
        : { stage: "reading", number, start: opening.start, times };
    } else if (opening.stage === "reading" && hasText) {
      this.#opening = { stage: "done", number };
      this.#happened({ at: this.#clock.now(), cause: { kind: "issue_text", number } });
      this.#whenPainted((painted) => {
        void opening.times.then((times) => {
          times.github = painted - opening.start;
          if (times.github > SLOW_FROM_GITHUB) {
            this.#misses.missed({ kind: "issue_from_github", number, ms: times.github });
          }
          this.issue = { stage: "read", number, fromDatabase: times.database, fromGitHub: times.github };
        });
      });
    }
  }

  /** Starts timing a jump in a PR's diff, hidden or shown. A jump the window hid during isn't timed, as its
   * time would hold the time it was hidden. */
  jumped(): Jump {
    const start = this.#clock.now();
    const hidings = this.#hidings;
    const recording = this.#recording;
    const times: JumpTimes = { rows: 0, spans: null };
    this.#lastJump = times;
    const show = () => {
      if (this.#lastJump === times) this.jump = { ...times };
    };
    const whenPainted = (callback: (painted: number) => void) => {
      this.#whenPainted((painted) => {
        if (this.#hidings === hidings) callback(painted);
      });
    };
    return {
      rowsShown: (highlighted) => {
        whenPainted((painted) => {
          times.rows = painted - start;
          if (highlighted) times.spans = times.rows;
          if (times.rows > BUDGETS.jump) this.#misses.missed({ kind: "jump", ms: times.rows });
          recording?.jumps.push({ rows: times.rows, plain: !highlighted });
          show();
        });
      },
      spansShown: () => {
        whenPainted((painted) => {
          times.spans = painted - start;
          show();
        });
      },
    };
  }

  /** Waits for a view query, and times it. */
  async timed<T>(ask: () => Promise<T>): Promise<T> {
    const start = this.#clock.now();
    const hidings = this.#hidings;
    try {
      return await ask();
    } finally {
      const answered = this.#clock.now();
      this.viewQuery = answered - start;
      // A hidden window's answers wait on the browser, so their time isn't the query's.
      const shown = !document.hidden && this.#hidings === hidings;
      if (shown && answered - start > BUDGETS.viewQuery) {
        this.#misses.missed({ kind: "view_query", ms: answered - start });
      }
      this.#recording?.viewQueries.push(answered - start);
      this.#happened({ at: answered, cause: { kind: "view_query" } });
    }
  }

  /** Asks the core how long it has been running, the first time a Workbench is usable. */
  async markUsable() {
    if (this.#startupSettled) return;
    this.#startupSettled = true;
    const usable = this.#clock.now();
    try {
      const running = await this.#core.timeRunning();
      // The core read its clock somewhere between the question and the answer, and the middle stands for it.
      const since = (this.#clock.now() - usable) / 2;
      this.startup = running === null ? undefined : running - since;
      if (this.startup !== undefined) this.#misses.startup(this.startup);
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Leaves this run without a start-up time, because it opened on something other than a Workbench. */
  skipStartup() {
    this.#startupSettled = true;
  }

  /**
   * Says whether a background refresh is running. The HUD counts the frames dropped during a refresh it saw
   * start while it showed.
   */
  refreshing(active: boolean) {
    if (active && this.#lastHeard === false) {
      this.#refreshDropped ??= 0;
    } else if (!active && this.#refreshDropped !== undefined) {
      this.lastRefresh = this.#refreshDropped;
      this.#recording?.refreshes.push(this.#refreshDropped);
      this.#refreshDropped = undefined;
    }
    if (this.visible) this.#lastHeard = active;
  }

  start() {
    if (!this.visible) return;
    this.#recording = {
      started: this.#clock.now(),
      frames: [],
      longFrames: [],
      keys: [],
      viewQueries: [],
      refreshes: [],
      issues: [],
      jumps: [],
    };
    this.recording = true;
    this.written = undefined;
    this.error = undefined;
  }

  /** Ends the recording, and writes what it saw to the log. */
  async stop() {
    const recording = this.#recording;
    if (!recording) return;
    this.#recording = undefined;
    this.recording = false;
    try {
      this.written = await this.#core.recordMeasurement(this.#measurement(recording));
      this.error = undefined;
    } catch (e) {
      this.error = String(e);
    }
  }

  #measurement(recording: Recording): Measurement {
    const frames = frameStats(recording.frames, BUDGETS.frame);
    return {
      seconds: (this.#clock.now() - recording.started) / 1000,
      frames: frames
        ? {
            count: frames.count,
            p95_ms: frames.p95,
            worst_ms: frames.worst,
            limit_ms: BUDGETS.frame,
            over_limit: frames.overLimit,
          }
        : null,
      startup_ms: this.startup ?? null,
      keys: slowest(recording.keys),
      view_queries: slowest(recording.viewQueries),
      refreshes:
        recording.refreshes.length === 0
          ? null
          : {
              count: recording.refreshes.length,
              dropped_frames: recording.refreshes.reduce((sum, dropped) => sum + dropped, 0),
            },
      issues: issueTimes(recording.issues),
      jumps: jumpTimes(recording.jumps),
      long_frames: recording.longFrames.map((frame) => ({
        at_s: (frame.end - recording.started) / 1000,
        ms: frame.duration,
        cause: frame.cause ?? null,
      })),
    };
  }

  #show() {
    this.visible = true;
    // The numbers from the last time it showed are old now.
    this.frames = undefined;
    this.key = undefined;
    this.lastRefresh = undefined;
    this.longFrame = undefined;
    this.issue = undefined;
    this.jump = undefined;
    this.#recent = [];
    this.#previous = undefined;
    window.addEventListener("keydown", this.#onKey, { capture: true });
    window.addEventListener("click", this.#onClick, { capture: true });
    this.#cancelPaintFrame();
    this.#handle = this.#clock.requestFrame(this.#onFrame);
  }

  /**
   * Hiding the HUD ends a recording, which still reaches the log. An Issue being opened is still timed, with
   * the one frame a hidden HUD asks for.
   */
  #hide() {
    if (this.#recording) void this.stop();
    this.visible = false;
    window.removeEventListener("keydown", this.#onKey, { capture: true });
    window.removeEventListener("click", this.#onClick, { capture: true });
    this.#clock.cancelFrame(this.#handle);
    this.#happenings = [];
    this.#refreshDropped = undefined;
    this.#lastHeard = undefined;
    this.#askForPaint();
  }

  #onKey = (e: KeyboardEvent) => {
    this.keyPressed(e.timeStamp);
  };

  #onClick = (e: MouseEvent) => {
    this.#happened({ at: e.timeStamp, cause: { kind: "click" } });
  };

  /** Notes what the window did, while the HUD shows, as a cause for a long frame after it. */
  #happened(happening: Happening) {
    if (this.visible) this.#happenings.push(happening);
  }

  /** Calls `callback` with the time once the next frame has been painted. */
  #whenPainted(callback: (at: number) => void) {
    this.#afterNextPaint.push(callback);
    this.#askForPaint();
  }

  /** A hidden HUD runs no frame loop, so it asks for the one frame that what waits for a paint needs. */
  #askForPaint() {
    if (this.visible || this.#paintHandle !== undefined || this.#afterNextPaint.length === 0) return;
    this.#paintHandle = this.#clock.requestFrame(() => {
      this.#paintHandle = undefined;
      this.#timePaint();
    });
  }

  #cancelPaintFrame() {
    if (this.#paintHandle === undefined) return;
    this.#clock.cancelFrame(this.#paintHandle);
    this.#paintHandle = undefined;
  }

  /** Has what waits for this frame's paint read the clock once it has been painted. */
  #timePaint() {
    const waiting = this.#afterNextPaint;
    if (waiting.length === 0) return;
    this.#afterNextPaint = [];
    this.#clock.afterPaint(() => {
      const painted = this.#clock.now();
      for (const callback of waiting) callback(painted);
    });
  }

  /** A hidden window draws no frames, so the gap until it shows again isn't one. */
  #onVisibility = () => {
    this.#previous = undefined;
    // A hidden window paints nothing, so whatever waits for a paint would count the time it was hidden.
    if (document.hidden) {
      this.#hidings += 1;
      this.#cancelPaintFrame();
      this.#afterNextPaint = [];
      this.#pendingKey = undefined;
      if (this.#opening?.stage !== "done") this.issue = undefined;
      this.#opening = undefined;
    }
  };

  #onFrame = (time: number) => {
    if (this.#previous === undefined) {
      this.#shownAt = time;
    } else {
      this.#addFrame({ end: time, duration: time - this.#previous });
    }
    this.#previous = time;
    this.#timePaint();
    if (time - this.#shownAt >= SHOW_EVERY) {
      this.#recent = recent(this.#recent, time, RECENT);
      this.#happenings = this.#happenings.filter((happening) => happening.at > time - CAUSES_KEPT);
      this.frames = frameStats(this.#recent, BUDGETS.frame);
      this.interval = frameInterval(this.#recent);
      this.#shownAt = time;
      this.now = time;
    }
    this.#handle = this.#clock.requestFrame(this.#onFrame);
  };

  #addFrame(frame: Frame) {
    this.#recent.push(frame);
    this.#recording?.frames.push(frame);
    if (frame.duration > BUDGETS.frame) {
      const long = { ...frame, cause: causeOf(frame, this.#happenings) };
      this.longFrame = long;
      this.#recording?.longFrames.push(long);
    }
    if (this.#refreshDropped !== undefined) {
      this.#refreshDropped += droppedFrames(frame.duration, this.interval);
    }
  }
}

/** The window's one HUD. */
export const hud = new HudState();
