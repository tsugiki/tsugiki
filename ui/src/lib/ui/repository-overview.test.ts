import { describe, expect, it } from "vitest";
import type { OnThisMachine, RepositorySettings } from "$lib/state/settings-dialog.svelte";
import { overviewLine } from "./repository-overview";

const filter = (name: string) => ({ name, query: "is:open", on_workbench: false });
const repository = (filters: number): RepositorySettings => ({
  name: "python/cpython",
  conventions: "cpython",
  filters: Array.from({ length: filters }, (_, at) => filter(`Filter ${String(at)}`)),
});
const machine = (source: OnThisMachine["source"], mirror: OnThisMachine["mirror"]): OnThisMachine => ({
  repository: "python/cpython",
  source,
  mirror,
});

describe("a Repository's line on Settings › Repositories", () => {
  it("counts its Filters before the core has measured its mirror", () => {
    expect(overviewLine(repository(0), undefined)).toBe("0 Filters");
    expect(overviewLine(repository(1), undefined)).toBe("1 Filter");
    expect(overviewLine(repository(2), undefined)).toBe("2 Filters");
  });

  it("says when its mirror borrows from a clone, and how much of the disk the mirror takes", () => {
    const borrow = { kind: "borrow", folder: "D:\\src\\cpython" } as const;
    const measured = { kind: "measured", mb: 1536 } as const;
    expect(overviewLine(repository(2), machine(borrow, measured))).toBe(
      "2 Filters · borrows from a clone · mirror 1.5 GB",
    );
    expect(overviewLine(repository(2), machine({ kind: "download" }, { kind: "measured", mb: 891 }))).toBe(
      "2 Filters · mirror 891 MB",
    );
  });

  it("says a mirror smaller than a megabyte isn't empty, and when there's no mirror yet", () => {
    expect(overviewLine(repository(1), machine(null, { kind: "measured", mb: 0 }))).toBe(
      "1 Filter · mirror under 1 MB",
    );
    expect(overviewLine(repository(1), machine(null, { kind: "missing" }))).toBe("1 Filter · no mirror yet");
  });

  it("gives no size for a mirror the core can't read", () => {
    expect(overviewLine(repository(1), machine(null, { kind: "unreadable" }))).toBe(
      "1 Filter · can't measure the mirror",
    );
  });
});
