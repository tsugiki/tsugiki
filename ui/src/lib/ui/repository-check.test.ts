import { expect, it } from "vitest";
import type { RepositoryCheck } from "$lib/state/add-repository.svelte";
import { describeCheck } from "./repository-check";

const gcmon: RepositoryCheck = {
  repository: { name: "some-maintainer/gcmon", conventions: "github", filters: [] },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
  access: "other",
};

const cpython: RepositoryCheck = {
  repository: { name: "python/cpython", conventions: "cpython", filters: [] },
  open_issues: 6935,
  open_prs: 2308,
  size: "large",
  window_days: 30,
  first_sync_items: 4754,
  first_sync_seconds: 546,
  first_sync_points: 223,
  access: "other",
};

it("describes a small Repository", () => {
  expect(describeCheck(gcmon)).toEqual([
    "10 open Issues, 3 open PRs.",
    "Small: Tsugiki loads everything open, plus what closed in the last 30 days.",
    "First sync, estimated: 48 Issues and PRs, under a minute, about 12 points of your 5,000 an hour.",
    "Conventions: GitHub.",
  ]);
});

it("describes a large Repository", () => {
  expect(describeCheck(cpython)).toEqual([
    "6935 open Issues, 2308 open PRs.",
    "Large: Tsugiki loads what changed in the last 30 days.",
    "First sync, estimated: 4754 Issues and PRs, about 9 minutes, about 223 points of your 5,000 an hour.",
    "Conventions: CPython.",
  ]);
});

it("counts one of each in the singular", () => {
  const one = {
    ...gcmon,
    open_issues: 1,
    open_prs: 1,
    first_sync_items: 1,
    first_sync_seconds: 60,
    first_sync_points: 1,
  };
  expect(describeCheck(one)).toEqual([
    "1 open Issue, 1 open PR.",
    "Small: Tsugiki loads everything open, plus what closed in the last 30 days.",
    "First sync, estimated: 1 Issue or PR, about 1 minute, about 1 point of your 5,000 an hour.",
    "Conventions: GitHub.",
  ]);
});
