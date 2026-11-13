import { expect, test } from "@playwright/test";
import type { Filter } from "../src/lib/data/bindings.ts";
import { changeAnswer, mockCore } from "./mock-core.ts";

const cpython = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const peps = { name: "python/peps", conventions: "github", filters: [] as Filter[] } as const;
const workbench = { items: [], quiet: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [] };

test("Ctrl+R and Enter switch back to the Repository opened before", async ({ page }) => {
  await mockCore(page, {
    home: { screen: "repository", repository: cpython, others: [peps.name, "python/devguide"] },
    openRepository: {
      status: "ok",
      data: { screen: "repository", repository: peps, others: [cpython.name] },
    },
    workbench: { status: "ok", data: workbench },
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "python/cpython" })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+R");
  const picker = page.getByRole("dialog", { name: "Repository picker" });
  await expect(picker.getByRole("option")).toHaveText([
    "python/cpython open",
    "python/peps",
    "python/devguide",
    "Add repository…",
  ]);
  await expect(picker.getByRole("option", { selected: true })).toHaveText("python/peps");
  await page.keyboard.press("Enter");

  await expect(picker).toBeHidden();
  await expect(page.getByRole("heading", { name: "python/peps" })).toBeVisible();
  await expect(page).toHaveTitle("python/peps · Tsugiki");

  // Back again: the core now lists cpython first among the others.
  await changeAnswer(page, "openRepository", {
    status: "ok",
    data: { screen: "repository", repository: cpython, others: [peps.name] },
  });
  await page.keyboard.press("ControlOrMeta+R");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "python/cpython" })).toBeVisible();
});
