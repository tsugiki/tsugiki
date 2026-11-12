import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { IssueViewState, type IssueView } from "$lib/state/issue-view.svelte";
import type { TreeEntry } from "$lib/state/workbench.svelte";
import IssueDetail from "./IssueDetail.svelte";

const core = vi.hoisted(() => ({
  issue: vi.fn(),
  openIssue: vi.fn(),
  fetchIssue: vi.fn(),
  openOnGithub: vi.fn(),
  openLink: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    issue: core.issue,
    openIssue: core.openIssue,
    fetchIssue: core.fetchIssue,
    openOnGithub: core.openOnGithub,
    openLink: core.openLink,
  },
  events: { changed: { listen: core.listen }, readFailed: { listen: core.listen } },
}));

const row = {
  title: "gh-7: Fix it",
  author: "some-author",
  state: "MERGED" as const,
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS" as const,
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: "APPROVED" as const,
};

const opened: TreeEntry = {
  kind: "pr",
  number: 13,
  ...row,
  state: "OPEN",
  your_review: null,
  reasons: [],
  backports: [],
};
const orphan: TreeEntry = { kind: "backport", number: 14, ...row, target_branch: "3.12", backport: "manual" };

const view: IssueView = {
  number: 7,
  title: "It crashes",
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  author: "issue-author",
  needs_attention: [],
  pull_requests: [
    {
      kind: "pr",
      number: 10,
      ...row,
      backports: [
        { number: 11, ...row, target_branch: "3.14", backport: "bot" },
        { number: 12, ...row, state: "OPEN", target_branch: "3.13", backport: "manual" },
      ],
    },
    opened,
    orphan,
  ],
  content: {
    body_html: "<p>The description</p>",
    comments: [
      {
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: '<p><a href="https://github.com/python/cpython/issues/1">A link</a></p>',
        hidden: null,
      },
      { author: null, created_at: "2026-11-04T10:00:00Z", body_html: "<p>Gone</p>", hidden: null },
    ],
    earlier_comments: 3,
  },
};

const NOW = new Date("2026-11-04T11:00:00Z");

const shown = async (answer: IssueView | null, onopenpr = vi.fn()) => {
  core.issue.mockResolvedValue({ status: "ok", data: answer });
  const state = new IssueViewState("python/cpython", 7);
  await state.load();
  render(IssueDetail, { repository: "python/cpython", issue: state, now: NOW, onopenpr });
  state.stop();
  return state;
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(vi.fn());
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.openLink.mockResolvedValue({ status: "ok", data: null });
});

it("shows the title and author, the description and the recent comments", async () => {
  await shown(view);

  expect(screen.getByRole("heading", { level: 2 }).textContent).toContain("#7 It crashes");
  expect(screen.getByText("issue-author")).toBeTruthy();
  expect(screen.getByText("The description").tagName).toBe("P");
  expect(screen.getByText("some-reviewer · 2h ago")).toBeTruthy();
  expect(screen.getByText("deleted user · 1h ago")).toBeTruthy();
  expect(screen.getByText("3 earlier comments are on GitHub.")).toBeTruthy();
});

it("counts one earlier comment as one", async () => {
  await shown({ ...view, content: { body_html: "<p>Short</p>", comments: [], earlier_comments: 1 } });

  expect(screen.getByText("1 earlier comment is on GitHub.")).toBeTruthy();
});

it("says nothing about earlier comments when it shows them all", async () => {
  await shown({ ...view, content: { body_html: "<p>Short</p>", comments: [], earlier_comments: 0 } });

  expect(screen.queryByText(/earlier comment/)).toBeNull();
});

it("shows each Origin PR's state, CI and review state, and folds its Backports into one line", async () => {
  await shown(view);

  expect(screen.getAllByText("Merged · CI passed · You approved")).toHaveLength(2);
  expect(screen.getByText("Open · CI passed")).toBeTruthy();
  const backports = screen.getByText(/Backports:/);
  expect(backports.textContent).toContain("3.14 Merged");
  expect(backports.textContent).toContain("3.13 Open");
});

it("counts the open PRs of all the PRs in the tree, Backports included", async () => {
  await shown(view);

  expect(screen.getByRole("heading", { level: 3 }).textContent).toBe("Origin PRs 2 open of 5 PRs");
});

it("counts one PR as one", async () => {
  await shown({ ...view, pull_requests: [orphan] });

  expect(screen.getByRole("heading", { level: 3 }).textContent).toBe("Origin PRs 0 open of 1 PR");
});

it("starts each PR's line with its branch and ends it with its author", async () => {
  await shown({ ...view, pull_requests: [opened, { ...orphan, author: null }] });

  const lines = within(screen.getByRole("list", { name: "Origin PRs" }))
    .getAllByRole("listitem")
    .map((item) => item.firstElementChild?.textContent?.trim());
  expect(lines).toEqual(["main #13 gh-7: Fix it some-author", "3.12 #14 gh-7: Fix it deleted user"]);
});

it("shows in bold each PR and Backport that Needs attention, and only those", async () => {
  await shown({ ...view, needs_attention: [12, 13] });

  const bold = (name: string) => screen.getByRole("button", { name }).classList.contains("attention");

  expect(bold("#13 gh-7: Fix it")).toBe(true);
  expect(bold("3.13 Open")).toBe(true);
  expect(bold("#10 gh-7: Fix it")).toBe(false);
  expect(bold("3.14 Merged")).toBe(false);
});

it("says under the tree what each icon and the bold mean", async () => {
  await shown(view);

  const legend = screen.getByText("Needs attention", { exact: false });
  const icons = within(legend.parentElement as HTMLElement).getAllByRole("img");

  expect(legend.parentElement?.textContent).toBe("OpenDraftMergedClosed Bold: Needs attention");
  expect(icons.map((icon) => icon.getAttribute("title"))).toEqual([
    "Open PR",
    "Draft PR",
    "Merged PR",
    "Closed PR",
  ]);
});

it("marks the Issue, each PR in its tree and each Backport with the icon of its state", async () => {
  await shown({ ...view, state: "CLOSED" });

  const heading = screen.getByRole("heading", { level: 2 });
  const tree = screen.getByRole("list", { name: "Origin PRs" });
  const icons = within(tree)
    .getAllByRole("img")
    .map((icon) => icon.getAttribute("title"));

  expect(heading.querySelector("[role=img]")?.getAttribute("title")).toBe("Closed issue");
  expect(icons).toEqual(["Merged PR", "Merged PR", "Open PR", "Open PR", "Merged PR"]);
});

it("leaves the icon out for an Issue Tsugiki stored before it kept states", async () => {
  await shown({ ...view, state: null });

  expect(within(screen.getByRole("heading", { level: 2 })).queryByRole("img")).toBeNull();
});

it("opens the title on GitHub", async () => {
  await shown(view);

  await fireEvent.click(screen.getByRole("button", { name: "#7 It crashes" }));

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", { kind: "issue", number: 7 });
});

it("opens a PR row and a Backport in the PR view", async () => {
  const onopenpr = vi.fn();
  await shown(view, onopenpr);

  await fireEvent.click(screen.getByRole("button", { name: "#10 gh-7: Fix it" }));
  await fireEvent.click(screen.getByRole("button", { name: "3.13 Open" }));
  await fireEvent.click(screen.getByRole("button", { name: "#14 gh-7: Fix it" }));

  expect(onopenpr.mock.calls).toEqual([[10], [12], [14]]);
  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("sends a link in a comment to the browser, and says if that fails", async () => {
  core.openLink.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await shown(view);

  await fireEvent.click(screen.getByRole("link", { name: "A link" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/issues/1");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("sends a link in the description to the browser", async () => {
  await shown({
    ...view,
    content: { body_html: '<a href="https://github.com/">Home</a>', comments: [], earlier_comments: 0 },
  });

  await fireEvent.click(screen.getByRole("link", { name: "Home" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/");
});

it("shows the tree before Tsugiki has read what the Issue says", async () => {
  await shown({ ...view, content: null, author: null, pull_requests: [] });

  expect(screen.getByText(/hasn't read what this Issue says/)).toBeTruthy();
  expect(screen.getByText("deleted user")).toBeTruthy();
  expect(screen.queryByRole("list")).toBeNull();
});

it("says so for an Issue Tsugiki doesn't hold", async () => {
  await shown(null);

  expect(screen.getByText("Tsugiki doesn't hold Issue #7.")).toBeTruthy();
});

// jsdom doesn't keep `scrollTop`, so `e2e/issue-view.spec.ts` checks that the pane comes back there.
it("keeps how far down the user scrolled its pane", async () => {
  const state = await shown(view);
  const pane = screen.getByRole("region", { name: "Issue view" });
  Object.defineProperty(pane, "scrollTop", { value: 300 });

  await fireEvent.scroll(pane);

  expect(state.scrollTop).toBe(300);
});

it("says why it can't show the Issue", async () => {
  core.issue.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  const state = new IssueViewState("python/cpython", 7);
  await state.load();
  render(IssueDetail, { repository: "python/cpython", issue: state, now: NOW, onopenpr: vi.fn() });
  state.stop();

  expect(screen.getByRole("alert").textContent).toContain("Tsugiki can't show this Issue: no database");
});
