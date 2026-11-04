import { expect, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";

const check = {
  repository: { name: "python/cpython", conventions: "cpython" },
  open_issues: 6935,
  open_prs: 2308,
  size: "large",
  window_days: 30,
  first_sync_items: 4754,
  first_sync_seconds: 546,
  first_sync_points: 223,
} as const;

const cpython = { screen: "repository", repository: check.repository } as const;
const onboarding = { screen: "onboarding", token: "gh" } as const;

test("adding the first Repository goes from onboarding to the Repository", async ({ page }) => {
  await mockCore(page, {
    home: onboarding,
    checkRepository: { status: "ok", data: check },
    addRepository: { status: "ok", data: cpython },
  });
  await page.goto("/");
  await expect(page.getByText("Your token comes from the GitHub CLI")).toBeVisible();

  await page.getByRole("button", { name: "Add repository" }).click();
  const input = page.getByRole("textbox", { name: "Repository" });
  await expect(input).toBeFocused();
  await input.fill("https://github.com/python/cpython");
  await input.press("Enter");

  const dialog = page.getByRole("dialog", { name: "Add repository" });
  await expect(dialog.getByText("6935 open Issues, 2308 open PRs.")).toBeVisible();
  await expect(dialog.getByText(/^Large:/)).toBeVisible();
  await expect(dialog.getByText(/about 9 minutes, about 223 points/)).toBeVisible();
  await expect(dialog.getByText("Conventions: CPython.")).toBeVisible();

  await input.press("Enter");
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("heading", { name: "python/cpython" })).toBeVisible();
});

test("a Repository GitHub doesn't have is named in the dialog, and Esc leaves onboarding as it was", async ({
  page,
}) => {
  const message = "GitHub has no Repository named python/no-such, or your token can't see it.";
  await mockCore(page, {
    home: onboarding,
    checkRepository: { status: "error", error: { kind: "not_found", message } },
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Add repository" }).click();
  await page.getByRole("textbox", { name: "Repository" }).fill("python/no-such");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("alert")).toHaveText(message);

  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(page.getByText("Your token comes from the GitHub CLI")).toBeVisible();
});

test("a normal launch opens on the Repository the core names", async ({ page }) => {
  await mockCore(page, { home: cpython });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "python/cpython" })).toBeVisible();
  await expect(page.getByText("Your token comes from")).toBeHidden();
});
