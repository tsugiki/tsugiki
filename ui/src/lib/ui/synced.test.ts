import { describe, expect, it } from "vitest";
import { cloningWords, elsewhereWords, syncedWords } from "./synced";

const now = new Date("2026-11-05T12:00:00Z");

describe("syncedWords", () => {
  it("says so before the first sync finishes", () => {
    expect(syncedWords(null, now)).toBe("Not synced yet");
  });

  it("calls the last minute just now, whatever the clocks say", () => {
    expect(syncedWords("2026-11-05T11:59:30Z", now)).toBe("Synced just now");
    expect(syncedWords("2026-11-05T12:00:30Z", now)).toBe("Synced just now");
  });

  it("counts minutes, then hours, then days", () => {
    expect(syncedWords("2026-11-05T11:58:00Z", now)).toBe("Synced 2 min ago");
    expect(syncedWords("2026-11-05T11:00:01Z", now)).toBe("Synced 59 min ago");
    expect(syncedWords("2026-11-05T09:00:00Z", now)).toBe("Synced 3 h ago");
    expect(syncedWords("2026-11-03T12:00:00Z", now)).toBe("Synced 2 d ago");
  });
});

describe("elsewhereWords", () => {
  const activity = (elsewhere: string[]) =>
    ({
      open: null,
      paused_until: null,
      elsewhere,
    }) as const;

  it("says nothing with no other Repository syncing", () => {
    expect(elsewhereWords(null)).toBeNull();
    expect(elsewhereWords(activity([]))).toBeNull();
  });

  it("names the one other Repository syncing", () => {
    expect(elsewhereWords(activity(["python/peps"]))).toBe("python/peps syncing");
  });

  it("names the next of several and counts the rest", () => {
    const several = ["python/peps", "python/devguide", "python/typeshed"];
    expect(elsewhereWords(activity(several))).toBe("python/peps and 2 more syncing");
  });
});

describe("cloningWords", () => {
  const repository = "python/cpython";

  it("says nothing with no clone under way", () => {
    expect(cloningWords(null)).toBeNull();
  });

  it.each([
    [{ stage: "starting" }, "Cloning python/cpython"],
    [{ stage: "receiving", percent: 42 }, "Cloning python/cpython 42%"],
    [{ stage: "resolving", percent: 80 }, "Indexing python/cpython 80%"],
  ] as const)("says the stage of %o and how far it has got", (progress, words) => {
    expect(cloningWords({ repository, progress })).toBe(words);
  });
});
