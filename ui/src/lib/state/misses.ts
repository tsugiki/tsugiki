// The budgets of ADR 0017 that the window misses in daily use (#151). Through 0.x the window logs each miss
// to `tsugiki.log` whether or not the performance HUD shows, so the log says where Tsugiki is slow without
// anyone starting a recording. None of it may cost an idle window anything: the HUD's timers run only around
// what the user does, and the browser reports slow keys and long frames on its own.
import { commands, type Miss, type MissKind, type Script } from "$lib/data/bindings";
import { frameInterval, nextFrameLimit } from "./frames";

/** The most misses of one kind the window logs in a minute, so a burst can't flood the log. */
export const CAP_PER_MINUTE = 10;

const MINUTE = 60_000;

/**
 * The longest a key or click may take to the next paint. While the HUD is hidden nothing counts the
 * display's frames, so this is a 60 Hz frame's limit.
 */
const INPUT_LIMIT = nextFrameLimit(frameInterval([]));

/**
 * The shortest event the browser reports. It rounds an event's duration to 8 ms, so this is the first
 * duration past `INPUT_LIMIT`, and the observer hears no event that can't be a miss.
 */
const EVENT_THRESHOLD = (Math.floor(INPUT_LIMIT / 8) + 1) * 8;

/** The last interactions the window logged, which it remembers so as to log each once. */
const INTERACTIONS_KEPT = 8;

/** The scripts a long frame's line names. */
const SCRIPTS_NAMED = 3;

/** A miss that can repeat, which the cap holds back. */
export type RepeatingMiss = Exclude<Miss, { kind: "startup" | "capped" }>;

/** The core's command that writes a miss to the log. */
export interface MissCore {
  recordMiss(miss: Miss): Promise<string>;
}

/** The parts of the browser's `PerformanceObserver` the window uses. */
export interface ObserverClass {
  new (callback: (list: { getEntries(): unknown[] }) => void): {
    observe(options: { type: string; durationThreshold?: number }): void;
  };
  readonly supportedEntryTypes: readonly string[];
}

/** The fields of a `PerformanceEventTiming` the window reads. `interactionId` is 0 for a mouse move. */
interface EventEntry {
  name: string;
  duration: number;
  interactionId: number;
}

/** The fields of a `PerformanceLongAnimationFrameTiming` the window reads. */
interface FrameEntry {
  duration: number;
  scripts: ScriptEntry[];
}

interface ScriptEntry {
  invoker: string;
  sourceFunctionName: string;
  sourceURL: string;
  duration: number;
}

/** The misses of one kind logged in the minute from `start`. */
interface Logged {
  start: number;
  count: number;
}

export class MissLog {
  #clock: { now(): number };
  #core: MissCore;
  #logged = new Map<MissKind, Logged>();
  #watching = false;

  constructor(clock: { now(): number }, core: MissCore = commands) {
    this.#clock = clock;
    this.#core = core;
  }

  /** Logs the time from Tsugiki starting to a usable Workbench, which a launch logs slow or not. */
  startup(ms: number) {
    this.#send({ kind: "startup", ms });
  }

  /** Logs `miss`, unless the minute's cap for its kind has been reached. */
  missed(miss: RepeatingMiss) {
    const now = this.#clock.now();
    const last = this.#logged.get(miss.kind);
    const logged = last === undefined || now - last.start >= MINUTE ? { start: now, count: 0 } : last;
    logged.count += 1;
    this.#logged.set(miss.kind, logged);
    if (logged.count <= CAP_PER_MINUTE) {
      this.#send(miss);
    } else if (logged.count === CAP_PER_MINUTE + 1) {
      this.#send({ kind: "capped", of: miss.kind, limit: CAP_PER_MINUTE });
    }
  }

  /**
   * Has the browser report slow keys and clicks, and long frames, where it can. WebView2 has both reports,
   * and WKWebView neither.
   */
  watch(Observer: ObserverClass | undefined) {
    // A second set of observers would log each miss twice.
    if (Observer === undefined || this.#watching) return;
    this.#watching = true;
    const supported = Observer.supportedEntryTypes;
    if (supported.includes("event")) {
      // A key's keydown and keyup are one interaction, as a click's pointerdown and click are, and the
      // entries of two interactions can come between each other's.
      let logged: number[] = [];
      new Observer((list) => {
        for (const entry of list.getEntries() as EventEntry[]) {
          const input = inputOf(entry.name);
          if (input === undefined || entry.interactionId === 0 || logged.includes(entry.interactionId))
            continue;
          if (entry.duration <= INPUT_LIMIT) continue;
          logged = [...logged, entry.interactionId].slice(-INTERACTIONS_KEPT);
          this.missed({ kind: "input", input, ms: entry.duration });
        }
      }).observe({ type: "event", durationThreshold: EVENT_THRESHOLD });
    }
    if (supported.includes("long-animation-frame")) {
      new Observer((list) => {
        for (const entry of list.getEntries() as FrameEntry[]) {
          this.missed({ kind: "long_frame", ms: entry.duration, scripts: longest(entry.scripts) });
        }
      }).observe({ type: "long-animation-frame" });
    }
  }

  /** The line fails only if the log can't be written, and the window has nowhere else to say so. */
  #send(miss: Miss) {
    this.#core.recordMiss(miss).catch(() => undefined);
  }
}

/** The input an event's name stands for, or nothing for an event no key or click starts. */
function inputOf(name: string): "key" | "click" | undefined {
  if (name.startsWith("key")) return "key";
  if (name.startsWith("pointer") || name.startsWith("mouse") || name === "click") return "click";
  return undefined;
}

/** The longest scripts of a frame, each named by its file's path. */
function longest(scripts: readonly ScriptEntry[]): Script[] {
  return scripts
    .toSorted((a, b) => b.duration - a.duration)
    .slice(0, SCRIPTS_NAMED)
    .map((script) => ({
      invoker: script.invoker,
      function: script.sourceFunctionName === "" ? null : script.sourceFunctionName,
      source: script.sourceURL === "" ? null : pathOf(script.sourceURL),
      ms: script.duration,
    }));
}

/** The path of `url`, without the window's origin, which is the same for each of Tsugiki's scripts. */
function pathOf(url: string): string {
  return URL.canParse(url) ? new URL(url).pathname : url;
}
