import { describe, expect, it } from "vitest";
import {
  cloningWords,
  elsewhereWords,
  mirrorWords,
  offlineCloneWords,
  questionWords,
  syncedWords,
} from "./synced";

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

describe("questionWords", () => {
  it("says the first Repository's mirror that waits for an answer", () => {
    expect(questionWords("python/cpython")).toBe("python/cpython's mirror waits for your answer");
  });
});

describe("mirrorWords", () => {
  const short = { kind: "disk_short", repository: "python/cpython", folder: "D:\\Tsugiki" } as const;

  it("says nothing while a mirror can be made", () => {
    expect(mirrorWords(null)).toBeNull();
  });

  it("says git is missing", () => {
    expect(mirrorWords({ kind: "no_git" })).toBe(
      "The Files tab needs git 2.30 or newer, and Tsugiki can't find git",
    );
  });

  it("names the old git in its own words", () => {
    expect(mirrorWords({ kind: "old_git", version: "git version 2.25.1" })).toBe(
      "The Files tab needs git 2.30 or newer: git version 2.25.1",
    );
  });

  it("gives the room a clone needs and has, and the folder", () => {
    expect(mirrorWords({ ...short, needed_mb: 6144, free_mb: 512 })).toBe(
      "No room to clone python/cpython: it needs 6.0 GB on the disk with D:\\Tsugiki, which has 512 MB free",
    );
  });

  it("counts gigabytes from one up", () => {
    expect(mirrorWords({ ...short, needed_mb: 1023, free_mb: 1536 })).toContain("needs 1023 MB");
    expect(mirrorWords({ ...short, needed_mb: 1023, free_mb: 1536 })).toContain("has 1.5 GB free");
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

describe("offlineCloneWords", () => {
  const offline = (repository: string) => ({ repository, state: { state: "offline" } }) as const;

  it("says nothing while no clone waits for the network", () => {
    expect(offlineCloneWords([])).toBeNull();
    expect(offlineCloneWords([{ repository: "python/cpython", state: { state: "ready" } }])).toBeNull();
  });

  it("names the one clone that waits for the network, and counts several", () => {
    expect(offlineCloneWords([offline("python/cpython")])).toBe(
      "Cloning python/cpython starts once GitHub answers",
    );
    expect(offlineCloneWords([offline("python/cpython"), offline("python/peps")])).toBe(
      "2 clones start once GitHub answers",
    );
  });
});
