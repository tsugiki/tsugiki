import { describe, expect, it } from "vitest";
import type { MirrorState } from "$lib/state/status.svelte";
import { mirrorNote } from "./mirror-note";

/** The note on cpython's row. */
const note = (state: MirrorState | undefined) => mirrorNote("python/cpython", state);

describe("mirrorNote", () => {
  it("says nothing of a ready mirror, or of one the core hasn't heard of", () => {
    expect(note({ state: "ready" })).toBeNull();
    expect(note(undefined)).toBeNull();
  });

  it("follows a clone as it moves on", () => {
    expect(note({ state: "cloning", progress: { stage: "starting" } })).toBe("cloning");
    expect(note({ state: "cloning", progress: { stage: "receiving", percent: 40 } })).toBe("cloning, 40%");
    expect(note({ state: "cloning", progress: { stage: "resolving", percent: 80 } })).toBe("indexing, 80%");
  });

  it("says a clone waits for the user's answer", () => {
    expect(note({ state: "asking" })).toBe("the mirror waits for your answer");
  });

  it("names the clone a mirror waits for, if one is under way", () => {
    expect(note({ state: "waiting", behind: "python/peps" })).toBe("waiting for python/peps's clone");
    expect(note({ state: "waiting", behind: null })).toBe("waiting to clone");
  });

  it("says a clone waits for the network", () => {
    expect(note({ state: "offline" })).toBe("cloning starts once GitHub answers");
  });

  it("says why the last clone failed, if git said", () => {
    expect(note({ state: "failed", words: "fatal: repository not found" })).toBe(
      "the clone failed: fatal: repository not found",
    );
    expect(note({ state: "failed", words: "" })).toBe("the clone failed");
  });

  it("names the problem that keeps a clone from starting", () => {
    expect(note({ state: "problem", problem: { kind: "no_git" } })).toBe("Tsugiki can't find git");
    expect(note({ state: "problem", problem: { kind: "old_git", version: "git version 2.29.2" } })).toBe(
      "needs git 2.30 or newer: git version 2.29.2",
    );
  });

  it("says how short the disk is on the row of the clone it is short for, and names that clone on the others", () => {
    const short: MirrorState = {
      state: "problem",
      problem: {
        kind: "disk_short",
        repository: "python/cpython",
        folder: "D:\\Tsugiki",
        needed_mb: 6144,
        free_mb: 512,
      },
    };
    expect(note(short)).toBe("no room on the disk: it needs 6.0 GB, 512 MB free");
    expect(mirrorNote("python/peps", short)).toBe("no room on the disk for python/cpython's clone");
  });
});
