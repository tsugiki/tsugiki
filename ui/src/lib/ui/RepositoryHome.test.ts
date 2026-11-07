import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import RepositoryHome from "./RepositoryHome.svelte";

const core = vi.hoisted(() => ({
  workbench: vi.fn(),
  issue: vi.fn(),
  openIssue: vi.fn(),
  fetchIssue: vi.fn(),
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  fetchPullRequest: vi.fn(),
  openOnGithub: vi.fn(),
  pullRequestFiles: vi.fn(),
  cloning: vi.fn(),
  // The HUD logs a slow opening of an Issue whether or not it shows.
  recordMiss: vi.fn(() => Promise.resolve("a miss")),
  listen: vi.fn(),
  stopListening: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    workbench: core.workbench,
    issue: core.issue,
    openIssue: core.openIssue,
    fetchIssue: core.fetchIssue,
    pullRequest: core.pullRequest,
    openPullRequest: core.openPullRequest,
    fetchPullRequest: core.fetchPullRequest,
    openOnGithub: core.openOnGithub,
    pullRequestFiles: core.pullRequestFiles,
    cloning: core.cloning,
    recordMiss: core.recordMiss,
  },
  events: {
    changed: { listen: core.listen },
    headsFetched: { listen: core.listen },
    cloningChanged: { listen: core.listen },
  },
}));

const row = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "some-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: ["review_request"],
  your_review: null,
};

const issue = {
  kind: "issue",
  number: 112564,
  title: "GIL drop during collection",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ kind: "pr", ...row, backports: [] }],
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(core.stopListening);
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.openPullRequest.mockResolvedValue({ status: "ok", data: null });
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  core.cloning.mockResolvedValue(null);
  core.pullRequest.mockImplementation((_repository: string, number: number) =>
    Promise.resolve({ status: "ok", data: { ...row, number, title: `PR ${String(number)}`, content: null } }),
  );
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      author: "issue-author",
      content: null,
    },
  });
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("names the Repository", () => {
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
});

it("offers to add another", async () => {
  const onadd = vi.fn();
  render(RepositoryHome, { name: "python/cpython", onadd });
  await fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(onadd).toHaveBeenCalledOnce();
});

it("lists a Standalone PR with the time it changed", async () => {
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ kind: "standalone_pr", ...row, backports: [] }],
      quiet: [],
      count: 1,
      review_requests: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const item = await screen.findByRole("listitem");

  expect(core.workbench).toHaveBeenCalledWith("python/cpython", { kind: "whole" });
  expect(within(item).getByText("#160662 gh-112564: Fix UAF for gc debug print")).toBeTruthy();
  expect(
    within(item).getByText("Review request · some-author · into main · Open · CI passed · 3h ago"),
  ).toBeTruthy();
});

it("moves a time on while the window stays open", async () => {
  // Only these, because the wait for the first answer runs on the real `setTimeout`.
  vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [issue], quiet: [], count: 1, review_requests: 0, filters: [] },
  });
  const { unmount } = render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  await screen.findByText("issue-author, some-author · 1 PR · 3h ago");

  await vi.advanceTimersByTimeAsync(60 * 60_000);

  expect(screen.getByText("issue-author, some-author · 1 PR · 4h ago")).toBeTruthy();
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("lists an Issue with its PRs under it", async () => {
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [issue], quiet: [], count: 1, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const [item, pullRequest] = await screen.findAllByRole("listitem");

  expect(within(item as HTMLElement).getByText("#112564 GIL drop during collection")).toBeTruthy();
  expect(within(item as HTMLElement).getByText("issue-author, some-author · 1 PR · 3h ago")).toBeTruthy();
  expect(
    within(pullRequest as HTMLElement).getByText("#160662 gh-112564: Fix UAF for gc debug print"),
  ).toBeTruthy();
  expect(
    within(pullRequest as HTMLElement).getByText(
      "Review request · some-author · into main · Open · CI passed",
    ),
  ).toBeTruthy();
});

it("says so while there is nothing to list", async () => {
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect(await screen.findByText(/Nothing here yet/)).toBeTruthy();
});

it("shows a Filter's Issues when the user selects it, with the Sidebar's counts", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", count: 1, problem: null };
  core.workbench.mockImplementation(async (_: string, selected: { kind: string }) => ({
    status: "ok",
    data: {
      items: selected.kind === "whole" ? [] : [issue],
      quiet: [],
      count: 7,
      review_requests: 0,
      filters: [mine],
    },
  }));
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 1" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "filter", query: mine.query });
  expect(await screen.findByText("#112564 GIL drop during collection")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Workbench 7" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Mine 1" }).getAttribute("aria-current")).toBe("true");
});

it("marks what Needs attention the way unread mail is, and says why", async () => {
  const attention = {
    since: "2026-11-04T10:00:00Z",
    why: [{ kind: "changes_requested", by: "some-reviewer" }, { kind: "review_requested" }],
  };
  const unread = { kind: "standalone_pr", ...row, backports: [], attention };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [unread], quiet: [], count: 1, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const item = await screen.findByRole("listitem");

  expect(within(item).getByText("#160662 gh-112564: Fix UAF for gc debug print").tagName).toBe("STRONG");
  expect(
    within(item).getByText("Changes requested by some-reviewer · Review requested from you"),
  ).toBeTruthy();
});

it("lists the Issues with nothing waiting apart, under Quiet", async () => {
  const standalone = { kind: "standalone_pr", ...row, number: 7, backports: [], attention: null };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [standalone], quiet: [issue], count: 0, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const heading = await screen.findByRole("heading", { name: "Quiet" });
  const after = heading.nextElementSibling as HTMLElement;

  expect(within(after).getByText("#112564 GIL drop during collection")).toBeTruthy();
  expect(screen.getByText("#7 gh-112564: Fix UAF for gc debug print").tagName).toBe("SPAN");
});

it("shows only the quiet section when nothing else is listed", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [issue], count: 0, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  expect(await screen.findByRole("heading", { name: "Quiet" })).toBeTruthy();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says when nobody has asked for a review", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Review requests 0" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "review_requests" });
  expect(await screen.findByText("Nobody has asked you for a review.")).toBeTruthy();
});

it("says how to check a Filter that holds nothing", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [mine] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 0" }));

  expect(await screen.findByText(/check its query in settings.toml/)).toBeTruthy();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says why it can't read the Workbench", async () => {
  core.workbench.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Tsugiki can't read this Workbench: no database",
  );
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("opens the other Repository's Workbench and stops listening for the last", async () => {
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  await screen.findByText(/Nothing here yet/);

  await rerender({ name: "tsugiki/tsugiki", onadd: () => {} });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("tsugiki/tsugiki", { kind: "whole" });
  });
  expect(core.stopListening).toHaveBeenCalledOnce();
});

it("stops listening when it goes", async () => {
  const { unmount } = render(RepositoryHome, { name: "python/cpython", onadd: () => {} });
  await screen.findByText(/Nothing here yet/);
  unmount();
  expect(core.stopListening).toHaveBeenCalledOnce();
});

const answering = (...items: unknown[]) => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items, quiet: [], count: items.length, review_requests: 0, filters: [] },
  });
};

const backportRow = {
  ...row,
  number: 160700,
  title: "[3.14] gh-112564: Fix UAF for gc debug print (GH-160662)",
  target_branch: "3.14",
  reasons: [],
  your_review: null,
  backport: "bot",
};

it("lists an Origin PR's Backports beneath it in the Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const origin = (await screen.findByText(/^#160662 /)).closest("li") as HTMLElement;

  const beneath = within(origin)
    .getByText(/^#160700 /)
    .closest("li") as HTMLElement;
  expect(within(beneath).getByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

it("lists a Backport with no Origin PR directly under its Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "backport", ...backportRow }] });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  expect(await screen.findByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

it("lists a Standalone PR's Backports beneath it", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  const origin = (await screen.findByText(/^#160662 /)).closest("li") as HTMLElement;

  expect(within(origin).getByText(/^#160700 /)).toBeTruthy();
});

it("lists a Backport with no Issue and no Origin PR by itself", async () => {
  answering({ kind: "standalone_backport", ...backportRow });
  render(RepositoryHome, { name: "python/cpython", onadd: () => {} });

  expect(await screen.findByText(/^#160700 /)).toBeTruthy();
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
});

const standalone = { kind: "standalone_pr", ...row, number: 5, title: "Alone", backports: [] };

/** A window on two items: the Issue and, under Quiet, a Standalone PR. */
const listing = async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [{ ...standalone, attention: null }],
      count: 0,
      review_requests: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });
  return {
    issueRow: await screen.findByRole("button", { name: "#112564 GIL drop during collection" }),
    treeRow: screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
    prRow: screen.getByRole("button", { name: "#5 Alone" }),
  };
};

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(document.body, { key, ...init });

it("opens an Issue in the Detail pane on a click, and moves Last seen", async () => {
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("says how long ago each comment in the Detail pane came", async () => {
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      author: "issue-author",
      content: {
        body_html: "<p>It crashes</p>",
        comments: [
          { author: "some-reviewer", created_at: new Date().toISOString(), body_html: "<p>Which build?</p>" },
        ],
        earlier_comments: 0,
      },
    },
  });
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);

  expect(await screen.findByText("some-reviewer · just now")).toBeTruthy();
});

it("opening the Issue on show again moves Last seen again", async () => {
  const { issueRow } = await listing();

  await fireEvent.click(issueRow);
  await fireEvent.click(issueRow);

  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenCalledTimes(2);
  });
});

it("swaps the Detail pane for the next Issue", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [
        { ...issue, attention: null },
        { ...issue, number: 2, title: "Next", attention: null },
      ],
      quiet: [],
      count: 0,
      review_requests: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });

  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await fireEvent.click(screen.getByRole("button", { name: "#2 Next" }));

  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenLastCalledWith("python/cpython", 2);
  });
});

it("opens a Standalone PR in the Detail pane on a click, on its Conversation, and moves Last seen", async () => {
  vi.setSystemTime("2026-11-04T12:00:00Z");
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: {
      ...standalone,
      content: {
        body_html: "<p>Why it fails</p>",
        comments: [{ author: "some-reviewer", created_at: "2026-11-04T10:00:00Z", body_html: "<p>LGTM</p>" }],
        earlier_comments: 0,
      },
    },
  });
  const { prRow } = await listing();

  await fireEvent.click(prRow);

  const conversation = await screen.findByRole("tab", { name: "Conversation" });
  expect(conversation.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByText("Why it fails")).toBeTruthy();
  expect(screen.getByText("some-reviewer · 2h ago")).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 5);
  expect(core.openOnGithub).not.toHaveBeenCalled();
  expect(prRow.getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens a PR in an Issue's tree with the way back to its Issue, and Esc goes back", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();

  await press("Escape");

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("opens a PR that fixes two Issues again from its line under the other one", async () => {
  answering(issue, { ...issue, number: 112565, title: "Another crash" });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });
  const [first, second] = await screen.findAllByRole("button", { name: /^#160662 / });

  await fireEvent.click(first as HTMLElement);
  await screen.findByRole("button", { name: "gh-112564" });
  await fireEvent.click(second as HTMLElement);

  expect(await screen.findByRole("button", { name: "gh-112565" })).toBeTruthy();
  expect((await screen.findByRole("tab", { name: "Conversation" })).getAttribute("aria-selected")).toBe(
    "true",
  );
  expect(second?.getAttribute("aria-current")).toBe("true");
  expect(first?.getAttribute("aria-current")).toBeNull();
});

it("goes back to the Issue with the PR view's gh-N link", async () => {
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);

  await fireEvent.click(await screen.findByRole("button", { name: "gh-112564" }));

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
});

it("opens a Backport in the list in the PR view, with its Issue to go back to", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160700);
});

it("opens a Standalone PR's Backport in the PR view, with no Issue to go back to", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("tab", { name: "Conversation" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens a PR from the Issue view, and puts the cursor on its line in the tree", async () => {
  core.issue.mockResolvedValue({
    status: "ok",
    data: { ...issue, author: "issue-author", content: null },
  });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  const issueView = await screen.findByRole("region", { name: "Issue view" });

  await fireEvent.click(await within(issueView).findByRole("button", { name: /^#160662 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
  expect(treeRow.getAttribute("aria-current")).toBe("true");
});

it("closes a Standalone PR's view with Esc, and leaves Esc alone with no PR on show", async () => {
  const { prRow, issueRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("tab", { name: "Conversation" });

  await press("Escape");
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(core.openIssue).not.toHaveBeenCalled();

  await fireEvent.click(issueRow);
  await press("Escape");
  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
});

it("switches the PR view's tabs with Ctrl+PageDown and Ctrl+PageUp", async () => {
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  const pageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  });

  await fireEvent(document.body, pageDown);
  expect(files.getAttribute("aria-selected")).toBe("true");
  expect(pageDown.defaultPrevented).toBe(true);
  await press("PageUp", { ctrlKey: true });
  expect(files.getAttribute("aria-selected")).toBe("false");
});

it("switches the Files tab between unified and side by side with Ctrl+Alt+U, for each PR after", async () => {
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [] },
  });
  // jsdom lays nothing out, so the diff's bind:clientHeight has nothing to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  const toggle = () =>
    new KeyboardEvent("keydown", { key: "u", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
  // The Conversation tab has no diff to switch.
  const onConversation = toggle();
  await fireEvent(document.body, onConversation);
  await fireEvent.click(files);
  expect(await screen.findByRole("region", { name: "Diff" })).toBeTruthy();

  const onFiles = toggle();
  await fireEvent(document.body, onFiles);
  expect(await screen.findByRole("region", { name: "Side-by-side diff" })).toBeTruthy();
  await press("Escape");
  await fireEvent.click(prRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));

  expect(await screen.findByRole("region", { name: "Side-by-side diff" })).toBeTruthy();
  expect([onConversation.defaultPrevented, onFiles.defaultPrevented]).toEqual([false, true]);
});

it("leaves Ctrl+PageDown alone with no PR on show", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const pageDown = new KeyboardEvent("keydown", {
    key: "PageDown",
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  });

  await fireEvent(document.body, pageDown);

  expect(pageDown.defaultPrevented).toBe(false);
});

it("switches tabs on a click and on Enter on the open PR's line, and reads it once", async () => {
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });

  await fireEvent.click(prRow);
  expect(files.getAttribute("aria-selected")).toBe("true");
  await press("Enter");
  expect(files.getAttribute("aria-selected")).toBe("false");
  expect(core.openPullRequest).toHaveBeenCalledOnce();
});

it("says so when the browser won't open", async () => {
  core.openOnGithub.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await listing();

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });

  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("moves through the list and the PRs in its trees with the arrows, and focus follows", async () => {
  const { issueRow, treeRow, prRow } = await listing();

  await press("ArrowDown");
  expect(document.activeElement).toBe(issueRow);
  await press("ArrowDown");
  expect(document.activeElement).toBe(treeRow);
  await press("ArrowDown");
  expect(document.activeElement).toBe(prRow);
  await press("ArrowUp");
  expect(document.activeElement).toBe(treeRow);
  expect(core.openIssue).not.toHaveBeenCalled();
  expect(core.openPullRequest).not.toHaveBeenCalled();
});

it("does nothing on the arrows with an empty list", async () => {
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });
  await screen.findByText(/Nothing here yet/);

  await press("ArrowDown");

  expect(document.activeElement).toBe(document.body);
});

it("opens the selection with Enter when focus has left the list", async () => {
  const { issueRow } = await listing();
  await press("ArrowDown");
  issueRow.blur();

  await press("Enter");

  expect(core.openIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
});

it("leaves Enter alone with nothing selected, and on a focused control", async () => {
  const { issueRow } = await listing();

  await press("Enter");
  await press("ArrowDown");
  await fireEvent.keyDown(issueRow, { key: "Enter" });

  expect(core.openIssue).not.toHaveBeenCalled();
});

it("takes a held Enter as one press", async () => {
  const { issueRow } = await listing();
  await press("ArrowDown");
  const repeat = new KeyboardEvent("keydown", {
    key: "Enter",
    repeat: true,
    bubbles: true,
    cancelable: true,
  });

  await fireEvent(issueRow, repeat);

  expect(repeat.defaultPrevented).toBe(true);
});

it("leaves the browser its other keys, and F12 for a development build", async () => {
  await listing();
  const f12 = new KeyboardEvent("keydown", { key: "F12", bubbles: true, cancelable: true });

  await fireEvent(document.body, f12);

  expect(f12.defaultPrevented).toBe(false);
});

it("Ctrl+Shift+G still works from a text box", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "G", ctrlKey: true, shiftKey: true });
  box.remove();

  expect(core.openOnGithub).toHaveBeenCalledOnce();
});

it("leaves the arrows to a text box", async () => {
  await listing();
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "ArrowDown" });
  box.remove();

  expect(document.activeElement).toBe(document.body);
});

it("leaves its keys alone while a dialog is on top", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [{ ...issue, attention: null }], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onadd: vi.fn(), keysActive: false });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });

  expect(document.activeElement).toBe(document.body);
  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("Ctrl+Shift+G opens the Issue on show on GitHub", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "issue",
    number: 112564,
  });
});

it("Ctrl+Shift+G opens what's under the cursor when no Issue is on show", async () => {
  await listing();

  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).not.toHaveBeenCalled();

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).toHaveBeenLastCalledWith("python/cpython", { kind: "issue", number: 112564 });

  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  expect(core.openOnGithub).toHaveBeenLastCalledWith("python/cpython", {
    kind: "pull_request",
    number: 160662,
  });
});

it("Ctrl+Shift+G opens the PR on show on GitHub", async () => {
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("button", { name: "gh-112564" });

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "pull_request",
    number: 160662,
  });
});

it("closes the Detail pane when the Repository changes", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [{ ...issue, attention: null }], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onadd: vi.fn() });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();

  await rerender({ name: "python/other" });

  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
});
