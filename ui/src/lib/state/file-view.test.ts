import { expect, it } from "vitest";
import type { DiffEnd } from "$lib/data/bindings";
import { diffShows, fileViewWords } from "./file-view";
import type { HitCode } from "./hit-code.svelte";

const place = (path: string, side: DiffEnd) => ({
  kind: "repository" as const,
  path,
  side,
  commit: "abcdef1234",
});

const edited = { kind: "lines" as const, added: 2, removed: 1 };

it("tells a file the diff shows lines of by its new path at the head and its old path at the base", () => {
  const files = [
    { path: "Lib/new.py", old_path: "Lib/old.py", change: edited },
    { path: "Lib/spam.py", old_path: null, change: edited },
    { path: "Lib/eggs.py", change: edited },
  ];
  const shows = (path: string, side: DiffEnd) => diffShows(files, place(path, side));

  expect(["Lib/new.py", "Lib/spam.py", "Lib/eggs.py"].map((path) => shows(path, "head"))).toEqual([
    true,
    true,
    true,
  ]);
  expect(["Lib/old.py", "Lib/spam.py", "Lib/eggs.py"].map((path) => shows(path, "base"))).toEqual([
    true,
    true,
    true,
  ]);
  expect([shows("Lib/old.py", "head"), shows("Lib/new.py", "base")]).toEqual([false, false]);
  expect(shows("Lib/other.py", "base")).toBe(false);
});

it("shows no lines of a file the PR only renames, or whose mode alone changes", () => {
  const none = { kind: "lines" as const, added: 0, removed: 0 };
  const files = [
    { path: "Lib/new.py", old_path: "Lib/old.py", change: none },
    { path: "Tools/run.py", old_path: null, change: none },
  ];

  expect(
    [place("Lib/old.py", "base"), place("Lib/new.py", "head"), place("Tools/run.py", "head")].map((at) =>
      diffShows(files, at),
    ),
  ).toEqual([false, false, false]);
});

it("names the commit and the side of the PR it is in the header", () => {
  const code = {} as HitCode;
  const words = (side: DiffEnd) => fileViewWords({ place: place("Lib/eggs.py", side), line: 4, code }, 11);

  expect(words("base")).toEqual({
    back: "← Back to PR #11's diff",
    commit: "at abcdef1 · the PR's base of PR #11",
  });
  expect(words("head").commit).toBe("at abcdef1 · the PR's head of PR #11");
});
