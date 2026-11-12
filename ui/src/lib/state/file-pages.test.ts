import { describe, expect, it } from "vitest";
import { FilePages, samePage, type Page } from "./file-pages";

const news: Page = { kind: "news" };
const gone: Page = { kind: "gone" };
const file = (at: number): Page => ({ kind: "file", file: at });

/** The pages from the first on, as `step` walks them. */
const walked = (pages: FilePages): Page[] => {
  const seen: Page[] = [];
  for (let page = pages.first; page !== undefined; page = pages.step(page, 1)) seen.push(page);
  return seen;
};

describe("FilePages", () => {
  it("puts the NEWS entries first, then each file in the diff's order, then the threads on files no longer in the diff", () => {
    const pages = new FilePages({ paths: ["a.c", "b.c"], news: true, gone: true });

    expect(walked(pages)).toEqual([news, file(0), file(1), gone]);
  });

  it("has only the files when the PR adds no NEWS entry and every thread is on a file it changes", () => {
    const pages = new FilePages({ paths: ["a.c", "b.c", "c.c"], news: false, gone: false });

    expect(walked(pages)).toEqual([file(0), file(1), file(2)]);
    expect(pages.has(news)).toBe(false);
    expect(pages.has(gone)).toBe(false);
    expect(pages.has(file(3))).toBe(false);
    expect(pages.has(file(-1))).toBe(false);
    expect(pages.step(gone, -1)).toBeUndefined();
  });

  it("goes back a page, and stops at either end", () => {
    const pages = new FilePages({ paths: ["a.c", "b.c"], news: true, gone: false });

    expect(pages.step(file(1), -1)).toEqual(file(0));
    expect(pages.step(file(0), -1)).toEqual(news);
    expect(pages.step(news, -1)).toBeUndefined();
    expect(pages.step(file(1), 1)).toBeUndefined();
  });

  it("has no pages for a diff with nothing in it", () => {
    const pages = new FilePages({ paths: [], news: false, gone: false });

    expect(pages.first).toBeUndefined();
  });

  it("finds a file's page by its path", () => {
    const pages = new FilePages({ paths: ["a.c", "Misc/NEWS.d/next/x.rst"], news: true, gone: false });

    expect(pages.ofPath("Misc/NEWS.d/next/x.rst")).toEqual(file(1));
    expect(pages.ofPath("gone.c")).toBeUndefined();
  });

  it("counts a file's place among the files from 1", () => {
    const pages = new FilePages({ paths: ["a.c", "b.c", "c.c"], news: true, gone: true });

    expect(pages.counted(file(1))).toEqual({ at: 2, count: 3, path: "b.c" });
    expect(pages.counted(news)).toBeUndefined();
    expect(pages.counted(gone)).toBeUndefined();
  });

  describe("carried", () => {
    const before = new FilePages({ paths: ["a.c", "b.c", "c.c"], news: true, gone: true });

    it("keeps the file on show in a newer diff by its path, wherever the newer diff lists it", () => {
      const after = new FilePages({ paths: ["new.c", "c.c"], news: false, gone: false });

      expect(after.carried(file(2), before)).toEqual(file(1));
    });

    it("starts from the first page when the newer diff no longer changes the file", () => {
      const after = new FilePages({ paths: ["c.c"], news: true, gone: false });

      expect(after.carried(file(0), before)).toEqual(news);
    });

    it("keeps the NEWS entries or the threads on files no longer in the diff while the newer diff has them", () => {
      const after = new FilePages({ paths: ["c.c"], news: true, gone: true });
      const without = new FilePages({ paths: ["c.c"], news: false, gone: false });

      expect(after.carried(news, before)).toEqual(news);
      expect(after.carried(gone, before)).toEqual(gone);
      expect(without.carried(gone, before)).toEqual(file(0));
    });
  });
});

describe("samePage", () => {
  it("tells pages apart by kind and by file", () => {
    expect(samePage(file(1), file(1))).toBe(true);
    expect(samePage(file(1), file(2))).toBe(false);
    expect(samePage(news, news)).toBe(true);
    expect(samePage(news, gone)).toBe(false);
    expect(samePage(news, undefined)).toBe(false);
  });
});
