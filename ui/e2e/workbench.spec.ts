import { expect, test } from "@playwright/test";
import type { WorkbenchRow_Serialize as WorkbenchRow } from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython" } as const;
const cpython = { screen: "repository", repository } as const;

const review: WorkbenchRow = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "johng",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  reasons: ["review_request"],
};

const own: WorkbenchRow = {
  number: 157178,
  title: "GH-156425: Clear GC frame on early return",
  author: "octocat",
  is_draft: true,
  target_branch: "3.14",
  ci: "FAILURE",
  reasons: ["own_pr"],
};

test("the Workbench lists what waits on the user", async ({ page }) => {
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: { rows: [review, own] } } });
  await page.goto("/");

  const rows = page.getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("#160662 gh-112564: Fix UAF for gc debug print");
  await expect(rows.nth(0)).toContainText("Review request · johng · into main · CI passed");
  await expect(rows.nth(1)).toContainText("Own PR · octocat · into 3.14 · Draft · CI failed");
});

test("the first sync's rows appear as the core saves them", async ({ page }) => {
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: { rows: [] } } });
  await page.goto("/");
  await expect(page.getByText(/Nothing here yet/)).toBeVisible();

  await changeAnswer(page, "workbench", { status: "ok", data: { rows: [review] } });
  await emitFromCore(page, "workbench-changed", { repository: "tsugiki/tsugiki" });
  await emitFromCore(page, "workbench-changed", { repository: repository.name });

  await expect(page.getByRole("listitem")).toHaveCount(1);
  await expect(page.getByText(/Nothing here yet/)).toBeHidden();
});

test("the Workbench says why it can't read", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: { status: "error", error: { kind: "internal", message: "The database stopped." } },
  });
  await page.goto("/");
  await expect(page.getByRole("alert")).toHaveText(
    "Tsugiki can't read this Workbench: The database stopped.",
  );
});
