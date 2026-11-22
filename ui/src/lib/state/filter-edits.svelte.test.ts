import { describe, expect, it } from "vitest";
import { FilterEdits, isBlank } from "./filter-edits.svelte";

const bugs = { name: "Bugs", query: "label:bug", on_workbench: true };
const docs = { name: "Docs", query: "label:docs", on_workbench: false };
const mine = { name: "Mine", query: "author:@me", on_workbench: false };

/** The Filters of python/cpython, as the files held them. */
const edits = () => {
  const edits = new FilterEdits();
  edits.take([{ name: "python/cpython", filters: [bugs, docs, mine] }]);
  return edits;
};
/** Each row's name. */
const names = (edits: FilterEdits) => edits.page("python/cpython")?.rows.map((row) => row.name);

describe("FilterEdits", () => {
  it("has no page for a Repository the files don't hold, and changes nothing as it starts", () => {
    const held = edits();
    expect(held.page("python/peps")).toBeUndefined();
    expect(held.changes).toEqual([]);
    expect(held.blank).toBe(false);
  });

  it("moves a Filter up and down, keeping the search each had in the files", () => {
    const held = edits();
    const page = held.page("python/cpython");
    page?.move(2, -1);
    expect(names(held)).toEqual(["Bugs", "Mine", "Docs"]);
    page?.move(0, 1);
    expect(names(held)).toEqual(["Mine", "Bugs", "Docs"]);
    expect(held.changes).toEqual([
      {
        repository: "python/cpython",
        filters: [
          { name: "Mine", query: "author:@me", was: "author:@me" },
          { name: "Bugs", query: "label:bug", was: "label:bug" },
          { name: "Docs", query: "label:docs", was: "label:docs" },
        ],
      },
    ]);
  });

  it("deletes a Filter, and adds one with a blank name and search, which is blank", () => {
    const held = edits();
    const page = held.page("python/cpython");
    page?.delete(1);
    page?.add();
    const added = page?.rows.at(-1);
    expect(names(held)).toEqual(["Bugs", "Mine", ""]);
    expect(added?.was).toBeNull();
    expect(held.blank).toBe(true);
    if (added === undefined) throw new Error("no row added");
    added.name = "Open";
    added.query = "  ";
    expect(page?.rows.map(isBlank)).toEqual([false, false, true]);
    added.query = "is:open";
    expect(held.blank).toBe(false);
  });

  it("counts a name or search back as the files said it as no change, and saves them without spaces round them", () => {
    const held = edits();
    const row = held.page("python/cpython")?.rows[0];
    if (row === undefined) throw new Error("no row");
    row.name = " Bugs ";
    expect(held.changes).toEqual([]);
    row.query = " label:bug is:open ";
    expect(held.changes[0]?.filters[0]).toEqual({
      name: "Bugs",
      query: "label:bug is:open",
      was: "label:bug",
    });
    row.query = "label:bug";
    expect(held.changes).toEqual([]);
  });

  it("makes a page for a Repository the files hold twice from the first, and one for a Repository added", () => {
    const held = new FilterEdits();
    held.take([
      { name: "python/cpython", filters: [bugs] },
      { name: "python/cpython", filters: [docs] },
    ]);
    held.add({ name: "python/peps", filters: [mine] });
    held.add({ name: "python/peps", filters: [] });
    expect(names(held)).toEqual(["Bugs"]);
    expect(held.page("python/peps")?.rows.map((row) => row.name)).toEqual(["Mine"]);
  });
});
