import { render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import { newsEntry } from "$lib/testing/fixtures";
import NewsEntries from "./NewsEntries.svelte";

it("says where each NEWS link goes in its tooltip", () => {
  render(NewsEntries, { news: [newsEntry()], onlink: vi.fn(), onfile: vi.fn() });

  expect(screen.getByRole("link", { name: "gh-1" }).getAttribute("title")).toBe(
    "Open in the browser: https://github.com/python/cpython/issues/1",
  );
  expect(screen.getByRole("link", { name: "PEP 8" }).getAttribute("title")).toBe(
    "Open in the browser: https://peps.python.org/pep-0008/",
  );
});
