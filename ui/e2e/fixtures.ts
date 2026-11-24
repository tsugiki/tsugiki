import { expect, type Locator, type Page } from "@playwright/test";
import type {
  Filter_Serialize as Filter,
  IssueView_Serialize as IssueView,
  ListedItem_Serialize as ListedItem,
  PrView_Serialize as PrView,
  Review_Serialize as Review,
} from "../src/lib/data/bindings.ts";
import { openOn, workbenchRow } from "../src/lib/testing/fixtures.ts";

// What the specs share: the Repository they open, the items they list, and the moves they make with the
// mouse and the palette. What the core answers is built in src/lib/testing/fixtures.ts, which the unit tests
// share too.

/** The Repository each spec opens, with no Filter. */
export const repository = {
  name: "python/cpython",
  conventions: "cpython",
  filters: [] as Filter[],
} as const;

/** What `home` answers when the app opens on `repository`, with no other Repository added. */
export const home = openOn(repository);

/** PR #10's row: open and unreviewed, its CI passed, seen. */
export const row = workbenchRow(10);

/** Issue #7, with PR #10 under it, seen, and with nothing waiting. */
export const issue: Extract<ListedItem, { kind: "issue" }> = {
  kind: "issue",
  number: 7,
  title: "Issue 7",
  state: "OPEN",
  people: ["issue-author"],
  issue_updated_at: "2026-11-04T10:00:00Z",
  mute_author: { kind: "mute", login: "issue-author" },
  pinned: false,
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ ...row, kind: "pr", backports: [] }],
  seen: { mark: { numbers: [7, 10], up_to: null, findings: [] }, state: "seen" },
  attention: null,
  key: { kind: "issue", number: 7 },
  hidden: null,
  unpin: "stays",
};

/** The user's own draft PR #157178 into 3.14, a Standalone PR with failed CI and nothing waiting. */
export const own: Extract<ListedItem, { kind: "standalone_pr" }> = {
  attention: null,
  key: { kind: "pull_request", number: 157178 },
  hidden: null,
  unpin: "stays",
  kind: "standalone_pr",
  ...workbenchRow(157178, {
    title: "GH-156425: Clear GC frame on early return",
    author: "octocat",
    is_draft: true,
    target_branch: "3.14",
    ci: "FAILURE",
    updated_at: "2026-11-04T09:00:00Z",
    reasons: ["own_pr"],
  }),
  backports: [],
};

/** What `issue` answers for Issue #7 before Tsugiki has read it from GitHub. */
export const issueView: IssueView = {
  ...issue,
  merged: [],
  merged_count: 0,
  author: "issue-author",
  assignees: [],
  labels: [],
  content: null,
  needs_attention: [],
};

/** What `pullRequest` answers for PR #10: its description says "The fix", and `reviews` are its Reviews. */
export const prView = (reviews: Review[] = []): PrView => ({
  ...row,
  content: {
    body_html: "<p>The fix</p>",
    comments: [],
    earlier_comments: 0,
    opened_at: null,
    reviews,
    earlier_reviews: 0,
  },
  shares_review_of: null,
  issues: [],
  hidden: null,
});

/** Runs palette command `title`, as the user picks it from the palette's commands. */
export async function palette(page: Page, title: string) {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  // The box opens with `>` typed, which marks the commands, and `fill` replaces it.
  await page.getByRole("combobox", { name: "Command" }).fill(`>${title}`);
  // One row, named for what it does: its key describes it, and stays out of its name.
  await expect(page.getByRole("option")).toHaveAccessibleName(title);
  await page.keyboard.press("Enter");
}

/** Takes hold of the pane edge `edge`, and gives the moves that drag it `by` pixels from where it was
 * taken, right if `by` is more than 0, and let go of it. */
export async function grabEdge(edge: Locator) {
  const { mouse } = edge.page();
  const box = (await edge.boundingBox()) ?? { x: 0, y: 0 };
  const [x, y] = [box.x + 8, box.y + 20];
  await mouse.move(x, y);
  await mouse.down();
  return {
    moveBy: (by: number) => mouse.move(x + by, y, { steps: 4 }),
    release: () => mouse.up(),
  };
}

/** Drags the pane edge `edge` `by` pixels, right if `by` is more than 0. */
export async function dragEdge(edge: Locator, by: number) {
  const grip = await grabEdge(edge);
  await grip.moveBy(by);
  await grip.release();
}
