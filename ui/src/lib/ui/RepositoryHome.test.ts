import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HitCode } from "$lib/state/hit-code.svelte";
import type { Peek } from "$lib/state/peek";
import { PrFilesState, type FilesMode } from "$lib/state/pr-files.svelte";
import type { ChordBindings } from "./keys";
import RepositoryHome from "./RepositoryHome.svelte";

const core = vi.hoisted(() => ({
  workbench: vi.fn(),
  issue: vi.fn(),
  openIssue: vi.fn(),
  fetchIssue: vi.fn(),
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  fetchPullRequest: vi.fn(),
  markSeen: vi.fn(),
  markUnseen: vi.fn(),
  openOnGithub: vi.fn(),
  pullRequestFiles: vi.fn(),
  pullRequestFind: vi.fn(),
  cloning: vi.fn(),
  mirrorStates: vi.fn(),
  mirrorsHeld: vi.fn(),
  paneLayout: vi.fn(),
  savePaneLayout: vi.fn(),
  // The HUD logs a slow opening of an Issue whether or not it shows.
  recordMiss: vi.fn(() => Promise.resolve("a miss")),
  listen: vi.fn(),
  stopListening: vi.fn(),
}));

// jsdom has no `inert`, which a browser reflects to the attribute, as Svelte sets the property.
Object.defineProperty(HTMLElement.prototype, "inert", {
  get(this: HTMLElement) {
    return this.hasAttribute("inert");
  },
  set(this: HTMLElement, inert: boolean) {
    this.toggleAttribute("inert", inert);
  },
});

vi.mock("$lib/data/bindings", () => ({
  commands: {
    workbench: core.workbench,
    issue: core.issue,
    openIssue: core.openIssue,
    fetchIssue: core.fetchIssue,
    pullRequest: core.pullRequest,
    openPullRequest: core.openPullRequest,
    fetchPullRequest: core.fetchPullRequest,
    markSeen: core.markSeen,
    markUnseen: core.markUnseen,
    openOnGithub: core.openOnGithub,
    pullRequestFiles: core.pullRequestFiles,
    pullRequestFind: core.pullRequestFind,
    cloning: core.cloning,
    mirrorStates: core.mirrorStates,
    mirrorsHeld: core.mirrorsHeld,
    paneLayout: core.paneLayout,
    savePaneLayout: core.savePaneLayout,
    recordMiss: core.recordMiss,
  },
  events: {
    changed: { listen: core.listen },
    readFailed: { listen: core.listen },
    headsFetched: { listen: core.listen },
    headsFetchFailed: { listen: core.listen },
    cloningChanged: { listen: core.listen },
    mirrorStatesChanged: { listen: core.listen },
    mirrorsHeldChanged: { listen: core.listen },
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
  your_review: { kind: "unreviewed" },
  seen: {
    mark: { numbers: [160662], up_to: "2026-11-04T10:00:00Z" },
    state: "new_activity",
  },
};

const issue = {
  kind: "issue",
  number: 112564,
  title: "GIL drop during collection",
  state: "OPEN",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ kind: "pr", ...row, backports: [] }],
  seen: {
    mark: { numbers: [112564, 160662], up_to: "2026-11-04T10:00:00Z" },
    state: "new_activity",
  },
};

const DEFAULT_LAYOUT = {
  sidebar: null,
  list: null,
  peek: null,
  sidebar_hidden: false,
  list_hidden: false,
  detail_only_hid: null,
  order: "attention",
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(core.stopListening);
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.openPullRequest.mockResolvedValue({ status: "ok", data: null });
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  core.pullRequestFind.mockResolvedValue({ status: "ok", data: { matches: [], more: false } });
  core.cloning.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.mirrorsHeld.mockResolvedValue(false);
  core.paneLayout.mockResolvedValue(DEFAULT_LAYOUT);
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULT_LAYOUT });
  core.pullRequest.mockImplementation((_repository: string, number: number) =>
    Promise.resolve({ status: "ok", data: { ...row, number, title: `PR ${String(number)}`, content: null } }),
  );
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.markSeen.mockResolvedValue({ status: "ok", data: null });
  core.markUnseen.mockResolvedValue({ status: "ok", data: null });
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      merged: [],
      merged_count: 0,
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      content: null,
      seen: issue.seen,
    },
  });
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("names the Repository", () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
});

it("offers to add another", async () => {
  const onadd = vi.fn();
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd });
  await fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(onadd).toHaveBeenCalledOnce();
});

it("opens the Repository picker from the name atop the Sidebar", async () => {
  const onpick = vi.fn();
  render(RepositoryHome, { name: "python/cpython", onpick, onadd: () => {} });
  await fireEvent.click(screen.getByRole("button", { name: "python/cpython" }));
  expect(onpick).toHaveBeenCalledOnce();
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
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  const item = await screen.findByRole("listitem");

  expect(core.workbench).toHaveBeenCalledWith("python/cpython", { kind: "whole" });
  expect(
    within(item).getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  ).toBeTruthy();
  expect(within(item).getByText("some-author")).toBeTruthy();
  expect(within(item).getByText("3h ago")).toBeTruthy();
  expect(within(item).getByText("Review request · some-author · into main · Open · CI passed")).toBeTruthy();
});

it("moves a time on while the window stays open", async () => {
  // Only these, because the wait for the first answer runs on the real `setTimeout`.
  vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
  vi.setSystemTime("2026-11-04T13:00:00Z");
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [issue], quiet: [], count: 1, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  const { unmount } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await screen.findByText("3h ago");

  await vi.advanceTimersByTimeAsync(60 * 60_000);

  expect(screen.getByText("4h ago")).toBeTruthy();
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("says an Issue has no PR yet, with no count of none", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, people: [], pull_requests: [] }],
      quiet: [],
      count: 1,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  const item = await screen.findByRole("listitem");

  expect([".people", ".count", ".facts"].map((part) => item.querySelector(part)?.textContent)).toEqual([
    "—",
    undefined,
    "No PR yet",
  ]);
});

it("lists an Issue folded, and its PRs under it once unfolded", async () => {
  vi.setSystemTime("2026-11-04T13:00:00Z");
  const attention = { since: "2026-11-04T10:00:00Z", why: [{ kind: "review_requested" }] };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, attention }],
      quiet: [],
      count: 1,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  const fold = await screen.findByRole("button", { name: "PRs of #112564", expanded: false });
  const folded = [screen.getAllByRole("listitem").length, fold.textContent];
  // The folded row still says why the Issue Needs attention, though the PR that does is folded away.
  expect(screen.getByText("Review requested from you")).toBeTruthy();

  await fireEvent.click(fold);
  const [item, pullRequest] = screen.getAllByRole("listitem");

  expect([...folded, fold.getAttribute("aria-expanded"), fold.textContent]).toEqual([1, "›", "true", "⌄"]);

  expect(
    within(item as HTMLElement).getByRole("button", { name: "#112564 GIL drop during collection" }),
  ).toBeTruthy();
  expect([".people", ".count", ".age"].map((part) => item?.querySelector(part)?.textContent)).toEqual([
    "issue-author, some-author",
    "1 PR",
    "3h ago",
  ]);
  expect(
    within(pullRequest as HTMLElement).getByRole("button", {
      name: "#160662 gh-112564: Fix UAF for gc debug print",
    }),
  ).toBeTruthy();
  expect(
    within(pullRequest as HTMLElement).getByText(
      "Review request · some-author · into main · Open · CI passed",
    ),
  ).toBeTruthy();
});

it("says so while there is nothing to list", async () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
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
      stale_reviews: 0,
      filters: [mine],
    },
  }));
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 1" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "filter", query: mine.query });
  expect(await screen.findByRole("button", { name: "#112564 GIL drop during collection" })).toBeTruthy();
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
    data: { items: [unread], quiet: [], count: 1, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  const item = await screen.findByRole("listitem");

  const title = within(item).getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" });
  expect(title.firstElementChild?.tagName).toBe("STRONG");
  expect(item.classList.contains("attention")).toBe(true);
  expect(
    within(item).getByText("Changes requested by some-reviewer · Review requested from you"),
  ).toBeTruthy();
});

it("folds the Issues with nothing waiting into a quiet section, and a click unfolds and folds it", async () => {
  const standalone = { kind: "standalone_pr", ...row, number: 7, backports: [], attention: null };
  const other = { ...issue, number: 8, title: "Another" };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [standalone],
      quiet: [issue, other],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  const line = await screen.findByRole("button", { name: "Show 2 quiet items" });
  expect(line.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("button", { name: "#112564 GIL drop during collection" })).toBeNull();
  const title = screen.getByRole("button", { name: "#7 gh-112564: Fix UAF for gc debug print" });
  expect(title.querySelector("strong")).toBeNull();
  expect(title.closest("li")?.classList.contains("attention")).toBe(false);

  await fireEvent.click(line);

  expect(line.textContent?.trim()).toBe("Hide 2 quiet items");
  expect(line.getAttribute("aria-expanded")).toBe("true");
  const quiet = screen.getByRole("list", { name: "Quiet items" });
  expect(within(quiet).getByRole("button", { name: "#112564 GIL drop during collection" })).toBeTruthy();

  await fireEvent.click(line);

  expect(line.textContent?.trim()).toBe("Show 2 quiet items");
  expect(screen.queryByRole("list", { name: "Quiet items" })).toBeNull();
});

it("shows only the quiet line when nothing else is listed, and counts one item as one", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [issue], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  expect(await screen.findByRole("button", { name: "Show 1 quiet item" })).toBeTruthy();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says when nobody has asked for a review", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Review requests 0" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "review_requests" });
  expect(await screen.findByText("Nobody has asked you for a review.")).toBeTruthy();
});

it("asks for Stale reviews when the user picks it, and says when no Review of theirs is stale", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, stale_reviews: 2, filters: [] },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Stale reviews 2" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "stale_reviews" });
  expect(await screen.findByRole("heading", { name: "Stale reviews" })).toBeTruthy();
  expect(await screen.findByText("No Review of yours is stale.")).toBeTruthy();
});

it("says how to check a Filter that holds nothing", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", count: 0, problem: null };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [mine] },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  await fireEvent.click(await screen.findByRole("button", { name: "Mine 0" }));

  expect(await screen.findByText(/check its query in settings.toml/)).toBeTruthy();
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("says why it can't read the Workbench", async () => {
  core.workbench.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Tsugiki can't read this Workbench: no database",
  );
  expect(screen.queryByText(/Nothing here yet/)).toBeNull();
});

it("opens the other Repository's Workbench and stops listening for the last", async () => {
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await screen.findByText(/Nothing here yet/);

  await rerender({ name: "tsugiki/tsugiki", onadd: () => {} });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("tsugiki/tsugiki", { kind: "whole" });
  });
  expect(core.stopListening).toHaveBeenCalledOnce();
});

it("stops listening when it goes", async () => {
  const { unmount } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await screen.findByText(/Nothing here yet/);
  unmount();
  expect(core.stopListening).toHaveBeenCalledOnce();
});

const answering = (...items: unknown[]) => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items, quiet: [], count: items.length, review_requests: 0, stale_reviews: 0, filters: [] },
  });
};

const backportRow = {
  ...row,
  number: 160700,
  title: "[3.14] gh-112564: Fix UAF for gc debug print (GH-160662)",
  target_branch: "3.14",
  reasons: [],
  your_review: { kind: "unreviewed" },
  backport: "bot",
};

it("lists an Origin PR's Backports beneath it in the Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await unfoldAll();

  const origin = (await screen.findByRole("button", { name: /^#160662 / })).closest("li") as HTMLElement;

  const beneath = within(origin)
    .getByRole("button", { name: /^#160700 / })
    .closest("li") as HTMLElement;
  expect(within(beneath).getByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

it("lists a Backport with no Origin PR directly under its Issue", async () => {
  answering({ ...issue, pull_requests: [{ kind: "backport", ...backportRow }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await unfoldAll();

  expect(await screen.findByText("Bot backport · some-author · into 3.14 · Open · CI passed")).toBeTruthy();
});

const staleReview = {
  kind: "stale_review",
  files: [
    { path: "Lib/ast.py", whitespace_only: false },
    { path: "Python/ast.c", whitespace_only: true },
  ],
};
const staleHover = "Changed since your Review: 2 files\nLib/ast.py\nPython/ast.c (whitespace only)";

it("shows a Stale review on a PR's row in the Issue's tree, with the files changed in its hover", async () => {
  const stale = { ...row, reasons: ["reviewed"], your_review: staleReview };
  answering({ ...issue, pull_requests: [{ kind: "pr", ...stale, backports: [] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await unfoldAll();

  const badge = await screen.findByText("Stale review");
  expect(badge.title).toBe(staleHover);
  expect(badge.closest(".facts")?.textContent).toBe(
    "Stale review · some-author · into main · Open · CI passed",
  );
});

it("shows the Origin PR's review state on each Bot backport's row beneath it", async () => {
  const backports = [
    { ...backportRow, your_review: staleReview },
    { ...backportRow, number: 160701, target_branch: "3.13", your_review: { kind: "reviewed" } },
  ];
  const origin = { kind: "pr", ...row, reasons: ["reviewed"], your_review: staleReview, backports };
  answering({ ...issue, pull_requests: [origin] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  await unfoldAll();

  const facts = (number: number) => {
    const button = screen.getByRole("button", { name: new RegExp(`^#${String(number)} `) });
    return button.closest(".row")?.querySelector(".facts") as HTMLElement;
  };
  await screen.findByRole("button", { name: /^#160700 / });
  expect(within(facts(160700)).getByText("Stale review").title).toBe(staleHover);
  expect(facts(160701).textContent).toBe(
    "Reviewed · Bot backport · some-author · into 3.13 · Open · CI passed",
  );
});

it.each([
  ["Standalone PR", { kind: "standalone_pr", ...row, backports: [] }],
  ["Backport with no Issue", { kind: "standalone_backport", ...row, backport: "manual" }],
])("shows a Stale review on a %s's row while nothing else about it waits", async (_, item) => {
  answering({ ...item, your_review: staleReview, attention: null });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  expect((await screen.findByText("Stale review")).title).toBe(staleHover);
});

it("lists a Standalone PR's Backports beneath it", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  const origin = (await screen.findByRole("button", { name: /^#160662 / })).closest("li") as HTMLElement;

  expect(within(origin).getByRole("button", { name: /^#160700 / })).toBeTruthy();
});

it("lists a Backport with no Issue and no Origin PR by itself", async () => {
  answering({ kind: "standalone_backport", ...backportRow });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });

  expect(await screen.findByRole("button", { name: /^#160700 / })).toBeTruthy();
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
});

const standalone = { kind: "standalone_pr", ...row, number: 5, title: "Alone", backports: [] };

/** A window on two items: the Issue, which it unfolds, and, in the quiet section it unfolds too, a
 * Standalone PR. */
const listing = async (settings: { diffFiles?: FilesMode } = {}) => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [{ ...standalone, attention: null }],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  const { component } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onadd: vi.fn(),
    ...settings,
  });
  const issueRow = await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  await unfoldAll();
  await fireEvent.click(screen.getByRole("button", { name: "Show 1 quiet item" }));
  return {
    component,
    issueRow,
    treeRow: screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
    prRow: screen.getByRole("button", { name: "#5 Alone" }),
  };
};

/** Unfolds each Issue the list shows, once it shows one, as each starts folded (#593). */
const unfoldAll = async () => {
  await screen.findAllByRole("button", { name: /^PRs of #/ });
  for (const fold of screen.queryAllByRole("button", { name: /^PRs of #/, expanded: false })) {
    await fireEvent.click(fold);
  }
};

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(document.body, { key, ...init });

/** How many of the listeners the page started haven't stopped. */
const listening = () => core.listen.mock.calls.length - core.stopListening.mock.calls.length;

/** Says a Repository changed to every listener the page started, as the core would. */
const catchChanged = () => {
  const callbacks: ((event: { payload: unknown }) => void)[] = [];
  core.listen.mockImplementation((callback: (event: { payload: unknown }) => void) => {
    callbacks.push(callback);
    return Promise.resolve(core.stopListening);
  });
  return (repository: string, items: number[] = []) => {
    for (const callback of callbacks) callback({ payload: { repository, items } });
  };
};

it("opens an Issue in the Detail pane on a click, and tells the core", async () => {
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
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      seen: issue.seen,
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

  expect((await screen.findByText("· just now")).parentElement?.textContent).toBe("some-reviewer · just now");
});

it("opening the Issue on show again tells the core again", async () => {
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
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });

  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await fireEvent.click(screen.getByRole("button", { name: "#2 Next" }));

  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenLastCalledWith("python/cpython", 2);
  });
});

it("opens a Standalone PR in the Detail pane on a click, on its Conversation, and tells the core", async () => {
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
  expect(screen.getByText("· 2h ago").parentElement?.textContent).toBe("some-reviewer · 2h ago");
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

it("keeps the merged PRs unfolded after one of them opens and Esc comes back", async () => {
  const merged = { kind: "pr", ...row, number: 150001, title: "gh-112564: An old fix", backports: [] };
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      number: 112564,
      title: "GIL drop during collection",
      state: "OPEN",
      people: [],
      updated_at: "2026-11-04T10:00:00Z",
      pull_requests: [],
      merged: [{ ...merged, state: "MERGED" }],
      merged_count: 1,
      author: "issue-author",
      assignees: [],
      labels: [],
      needs_attention: [],
      content: null,
      seen: issue.seen,
    },
  });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(await screen.findByRole("button", { name: "Show 1 merged PR" }));
  await fireEvent.click(screen.getByRole("button", { name: "#150001 gh-112564: An old fix" }));
  await screen.findByRole("region", { name: "PR view" });

  await press("Escape");

  expect(await screen.findByRole("button", { name: "Hide 1 merged PR" })).toBeTruthy();
  expect(screen.getByRole("list", { name: "Merged PRs" })).toBeTruthy();
});

it("opens a PR that fixes two Issues again from its line under the other one", async () => {
  answering(issue, { ...issue, number: 112565, title: "Another crash" });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await unfoldAll();
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

it("goes back to the Issue with the PR view's gh-N link, and focuses the Issue's row", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);

  await fireEvent.click(await screen.findByRole("button", { name: "gh-112564" }));

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
});

it.each([
  ["the PR's row", (treeRow: HTMLElement) => treeRow],
  ["a PR tab", () => screen.getByRole("tab", { name: "Files" })],
  [
    "the Sidebar",
    () =>
      within(screen.getByRole("navigation", { name: "Sidebar" })).getByRole("button", { name: /^Workbench/ }),
  ],
])("focuses the Issue's row on Esc from %s", async (_, focused) => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("tab", { name: "Files" });
  const from = focused(treeRow) as HTMLElement;
  from.focus();

  await fireEvent.keyDown(from, { key: "Escape" });

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  expect(issueRow.getAttribute("aria-current")).toBe("true");
});

it("keeps the Issue view while a PR from its tree is on show, and shows it again at once", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("heading", { name: /GIL drop during collection/ });

  await fireEvent.click(treeRow);
  await press("Escape");
  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("button", { name: "gh-112564" }));
  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await fireEvent.click(treeRow);
  await press("ArrowLeft", { altKey: true });

  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledOnce();
  expect(core.issue).toHaveBeenCalledOnce();
  expect(core.openPullRequest).toHaveBeenCalledTimes(3);
});

it("shows the Issue view it keeps at once on a click on the Issue's line, and tells the core", async () => {
  const send = catchChanged();
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("heading", { name: /GIL drop during collection/ });
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  send("python/cpython");

  await fireEvent.click(issueRow);

  expect(screen.getByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  await vi.waitFor(() => {
    expect(core.openIssue).toHaveBeenCalledTimes(2);
  });
  expect(core.issue).toHaveBeenCalledTimes(2);
});

it("opens the Issue afresh on the way back when its view couldn't read it", async () => {
  core.issue.mockResolvedValueOnce({ status: "error", error: { kind: "internal", message: "no database" } });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByText(/no database/);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  await press("Escape");

  expect(await screen.findByRole("heading", { name: /GIL drop during collection/ })).toBeTruthy();
  expect(core.openIssue).toHaveBeenCalledTimes(2);
});

it("asks GitHub nothing for the Issue view it keeps, and stops it when the PR gives way to another", async () => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  core.fetchIssue.mockResolvedValue({ status: "ok", data: null });
  core.fetchPullRequest.mockResolvedValue({ status: "ok", data: null });
  const { issueRow, treeRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const withIssue = listening();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  await vi.advanceTimersByTimeAsync(60_000);
  expect(core.fetchIssue).not.toHaveBeenCalled();
  expect(core.fetchPullRequest).toHaveBeenCalledOnce();
  await fireEvent.click(prRow);
  // With the kept view stopped, the Issue opens afresh and listens in its place.
  await fireEvent.click(issueRow);

  expect(core.openIssue).toHaveBeenCalledTimes(2);
  expect(listening()).toBe(withIssue);
});

it("stops the Issue view it keeps when it goes", async () => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });

  cleanup();

  expect(listening()).toBe(0);
});

it("opens a Backport in the list in the PR view, with its Issue to go back to", async () => {
  answering({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [backportRow] }] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await unfoldAll();

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160700);
});

it("opens a Standalone PR's Backport in the PR view, with no Issue to go back to", async () => {
  answering({ kind: "standalone_pr", ...row, backports: [backportRow] });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });

  await fireEvent.click(await screen.findByRole("button", { name: /^#160700 / }));

  expect(await screen.findByRole("tab", { name: "Conversation" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens a PR from the Issue view, and puts the cursor on its line in the tree", async () => {
  core.issue.mockResolvedValue({
    status: "ok",
    data: {
      ...issue,
      merged: [],
      merged_count: 0,
      author: "issue-author",
      assignees: [],
      labels: [],
      content: null,
      needs_attention: [],
    },
  });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  const issueView = await screen.findByRole("region", { name: "Issue view" });

  await fireEvent.click(await within(issueView).findByRole("button", { name: /^#160662 / }));

  expect(await screen.findByRole("button", { name: "gh-112564" })).toBeTruthy();
  expect(core.openPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
  expect(treeRow.getAttribute("aria-current")).toBe("true");
});

it.each([
  ["a Standalone PR", standalone, "#5 Alone"],
  ["a Standalone PR's Backport", { ...standalone, backports: [backportRow] }, /^#160700 /],
  ["a Backport with no Issue", { kind: "standalone_backport", ...backportRow }, /^#160700 /],
])("leaves %s on show on Esc, with no Issue under it", async (_, item, name) => {
  answering(item);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const prRow = await screen.findByRole("button", { name });
  await fireEvent.click(prRow);
  await screen.findByRole("tab", { name: "Conversation" });
  const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });

  await fireEvent(document.body, escape);

  expect(escape.defaultPrevented).toBe(false);
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(prRow.getAttribute("aria-current")).toBe("true");
  expect(core.openPullRequest).toHaveBeenCalledOnce();
  expect(core.openIssue).not.toHaveBeenCalled();
});

it("leaves Esc alone with an Issue on show", async () => {
  const { issueRow } = await listing();
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
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [], threads: [] },
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
  const { prRow, treeRow } = await listing();
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
  await fireEvent.click(treeRow);
  await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));

  expect(await screen.findByRole("region", { name: "Side-by-side diff" })).toBeTruthy();
  expect([onConversation.defaultPrevented, onFiles.defaultPrevented]).toEqual([false, true]);
});

it("moves through a PR's open threads with F8 and Shift+F8 on its Files tab", async () => {
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [], threads: [] },
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
  const next = vi.spyOn(PrFilesState.prototype, "nextThread");
  const { prRow } = await listing();
  await fireEvent.click(prRow);
  const files = await screen.findByRole("tab", { name: "Files" });
  // The Conversation tab has no threads to move through.
  await press("F8");
  await fireEvent.click(files);

  await press("F8");
  await press("F8", { shiftKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

/** Opens the Files tab of the Standalone PR #5, whose diff is empty, and returns the tab. */
const onFiles = async () => {
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [], threads: [] },
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
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  return screen.findByRole("tab", { name: "Files" });
};

it("moves through a PR's changes with F7 and Shift+F7 on its Files tab", async () => {
  const next = vi.spyOn(PrFilesState.prototype, "nextChange");
  const files = await onFiles();
  // The Conversation tab has no changes to move through.
  await press("F7");
  await fireEvent.click(files);

  await press("F7");
  await press("F7", { shiftKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

it("moves between a PR's files with Alt+PageDown and Alt+PageUp on its Files tab", async () => {
  const next = vi.spyOn(PrFilesState.prototype, "nextFile");
  const files = await onFiles();
  // The Conversation tab has no files to move through.
  await press("PageDown", { altKey: true });
  await fireEvent.click(files);

  await press("PageDown", { altKey: true });
  await press("PageUp", { altKey: true });

  expect(next.mock.calls).toEqual([[1], [-1]]);
});

it("opens a PR's Files tab one file at a time as the settings say, and switches it with Ctrl+K O and the palette", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [], threads: [] },
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const { component, treeRow } = await listing({ diffFiles: "one" });
  await fireEvent.click(treeRow);
  const tab = await screen.findByRole("tab", { name: "Files" });
  const command = (title: string) => component.paletteCommands().find((found) => found.title === title);
  const pressed = () =>
    screen.getByRole("button", { name: "One file at a time" }).getAttribute("aria-pressed");
  // The Conversation tab shows no files to switch.
  component.chordBindings()["ctrl+k o"]?.();
  expect([command("View: All Files"), command("View: One File at a Time")]).toEqual([undefined, undefined]);
  await fireEvent.click(tab);
  // The tab reads the diff the first time it shows.
  await screen.findByRole("region", { name: "Diff" });
  expect(pressed()).toBe("true");

  expect(command("View: All Files")?.note).toBe("Ctrl+K O");
  command("View: All Files")?.run();
  await tick();
  expect([pressed(), command("View: One File at a Time")?.title]).toEqual([
    "false",
    "View: One File at a Time",
  ]);
  component.chordBindings()["ctrl+k o"]?.();
  await tick();
  expect(pressed()).toBe("true");
});

it("unfolds the files unchanged since a Stale review with Ctrl+K J and the palette, on the Files tab", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const unfold = vi.spyOn(PrFilesState.prototype, "unfoldUnchanged").mockImplementation(() => {});
  const folded = vi.spyOn(PrFilesState.prototype, "foldedUnchanged", "get").mockReturnValue(2);
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news: [], threads: [] },
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const { component, treeRow } = await listing();
  await fireEvent.click(treeRow);
  const tab = await screen.findByRole("tab", { name: "Files" });
  const command = () =>
    component.paletteCommands().find((found) => found.title === "View: Unfold Unchanged Files");
  // The Conversation tab shows no files to unfold.
  component.chordBindings()["ctrl+k j"]?.();
  expect(command()).toBeUndefined();
  await fireEvent.click(tab);
  await screen.findByRole("region", { name: "Diff" });

  expect(command()?.note).toBe("Ctrl+K J");
  command()?.run();
  component.chordBindings()["ctrl+k j"]?.();
  expect(unfold).toHaveBeenCalledTimes(2);
  // Once no unchanged file is folded, the palette has nothing to offer.
  folded.mockReturnValue(0);
  expect(command()).toBeUndefined();
});

it("opens the search in the diff with Ctrl+F on the Files tab, and gives its box focus again from it", async () => {
  const files = await onFiles();
  await press("f", { ctrlKey: true });
  expect(screen.queryByRole("search")).toBeNull();
  await fireEvent.click(files);
  // The tab reads the diff the first time it shows.
  await screen.findByRole("region", { name: "Diff" });

  await press("f", { ctrlKey: true });
  const box = screen.getByRole<HTMLInputElement>("textbox", { name: "Find" });
  await fireEvent.input(box, { target: { value: "gc" } });
  await fireEvent.keyDown(box, { key: "f", ctrlKey: true });

  expect(screen.getByRole("search", { name: "Find in the diff" })).toBeTruthy();
  expect(document.activeElement).toBe(box);
  expect([box.selectionStart, box.selectionEnd]).toEqual([0, 2]);
});

it("closes the search with Escape from anywhere, then leaves the PR with the focus on the Issue's row", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  diff.focus();

  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("search")).toBeNull();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: /^#112564 / }));
});

it("opens the Definition peek with Alt+F12 on the Files tab, once for a held key", async () => {
  const peekDefinition = vi.spyOn(PrFilesState.prototype, "peekDefinition").mockResolvedValue();
  const files = await onFiles();
  await press("F12", { altKey: true });
  expect(peekDefinition).not.toHaveBeenCalled();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });

  await press("F12", { altKey: true });
  await press("F12", { altKey: true, repeat: true });

  expect(peekDefinition).toHaveBeenCalledTimes(1);
});

it("goes to the definition with F12 on the Files tab, once for a held key", async () => {
  const goToDefinition = vi.spyOn(PrFilesState.prototype, "goToDefinition").mockResolvedValue();
  const files = await onFiles();
  await press("F12");
  expect(goToDefinition).not.toHaveBeenCalled();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });

  await press("F12");
  await press("F12", { repeat: true });

  expect(goToDefinition).toHaveBeenCalledTimes(1);
});

/** A PR's Files tab with the file view over it, whose code holds 10 lines it asks the core for none of. */
const onFileView = async () => {
  const place = { kind: "repository" as const, path: "Lib/eggs.py", side: "base" as const, commit: "base" };
  const code = { count: 10, widest: 0, show: () => {}, line: () => "x = 1" } as unknown as HitCode;
  vi.spyOn(PrFilesState.prototype, "fileView", "get").mockReturnValue({ place, line: 4, code });
  const files = await onFiles();
  await fireEvent.click(files);
  return screen.findByRole("region", { name: "File view" });
};

it("leaves the diff's keys alone while the file view covers it, and the cursor on the PR's row", async () => {
  const goToDefinition = vi.spyOn(PrFilesState.prototype, "goToDefinition").mockResolvedValue();
  const nextChange = vi.spyOn(PrFilesState.prototype, "nextChange");
  await onFileView();
  const row = document.querySelector('[data-item="112564#160662"]');

  await press("F12");
  await press("F7");

  expect(goToDefinition).not.toHaveBeenCalled();
  expect(nextChange).not.toHaveBeenCalled();
  expect(row?.getAttribute("aria-current")).toBe("true");
});

it("closes the Definition peek with Escape before the search, and the search before the PR", async () => {
  const shown: { peek: Peek | undefined } = {
    peek: {
      at: { kind: "row", row: 2 },
      symbol: "gc",
      side: "head",
      commit: "abc",
      lookup: { state: "looking" },
    },
  };
  vi.spyOn(PrFilesState.prototype, "peek", "get").mockImplementation(() => shown.peek);
  const closePeek = vi.spyOn(PrFilesState.prototype, "closePeek").mockImplementation(() => {
    shown.peek = undefined;
  });
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  diff.focus();

  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(closePeek).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("search")).toBeTruthy();
  await fireEvent.keyDown(diff, { key: "Escape" });
  expect(screen.queryByRole("search")).toBeNull();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(closePeek).toHaveBeenCalledTimes(1);
});

it("stops Go to definition with Escape while it searches, before the PR closes", async () => {
  const closePeek = vi.spyOn(PrFilesState.prototype, "closePeek").mockImplementation(() => {});
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  vi.spyOn(PrFilesState.prototype, "going", "get").mockReturnValue(true);

  await fireEvent.keyDown(diff, { key: "Escape" });

  expect(closePeek).toHaveBeenCalledOnce();
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("leaves the focus where it is when the Files tab is drawn again with the search open", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  await screen.findByRole("region", { name: "Diff" });
  await press("f", { ctrlKey: true });
  const conversation = screen.getByRole("tab", { name: "Conversation" });
  await fireEvent.click(conversation);
  conversation.focus();

  await fireEvent.click(files);

  expect(screen.getByRole("search")).toBeTruthy();
  expect(document.activeElement).toBe(conversation);
});

it("goes back and forward through the Issues and PRs it opened with Alt+← and Alt+→", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });

  await press("ArrowLeft", { altKey: true });
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  await press("ArrowLeft", { altKey: true });
  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  await press("ArrowRight", { altKey: true });
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  await press("ArrowRight", { altKey: true });
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("goes back and forward with the arrows over the Detail pane, which name their target", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  const history = within(await screen.findByRole("navigation", { name: "History" }));
  const back = history.getByRole("button", { name: "Back" });
  const forward = history.getByRole("button", { name: "Forward" });
  await vi.waitFor(() => {
    expect(back.getAttribute("title")).toBe("Back to #112564 GIL drop during collection (Alt+←)");
  });
  expect(forward.hasAttribute("disabled")).toBe(true);
  expect(forward.getAttribute("title")).toBe("Forward (Alt+→): nothing there");

  await fireEvent.click(back);

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  const now = within(screen.getByRole("navigation", { name: "History" }));
  expect(now.getByRole("button", { name: "Back" }).hasAttribute("disabled")).toBe(true);
  expect(now.getByRole("button", { name: "Forward" }).getAttribute("title")).toBe(
    "Forward to #5 PR 5 (Alt+→)",
  );
  expect(now.getByText("#112564 GIL drop during collection")).toBeTruthy();
});

it("lists the recent items back on a right-click, and opens the one picked with the cursor on its row", async () => {
  const { issueRow, prRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  const history = within(screen.getByRole("navigation", { name: "History" }));

  await fireEvent.contextMenu(history.getByRole("button", { name: "Back" }));
  const menu = within(await screen.findByRole("menu", { name: "Recent items back" }));
  const items = menu.getAllByRole("menuitem").map((item) => item.textContent);
  expect(items).toHaveLength(2);
  expect(items[1]).toBe("#112564 GIL drop during collection");

  await fireEvent.click(menu.getByText("#112564 GIL drop during collection"));

  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(issueRow.getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("menu")).toBeNull();
});

it("goes back and forward on the mouse's own buttons, and keeps the page where it is", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });

  const pressed = new MouseEvent("mousedown", { button: 3, cancelable: true });
  window.dispatchEvent(pressed);
  const released = new MouseEvent("mouseup", { button: 3, cancelable: true });
  window.dispatchEvent(released);

  expect([pressed.defaultPrevented, released.defaultPrevented]).toEqual([true, true]);
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  await fireEvent.mouseUp(window, { button: 4 });
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  const other = new MouseEvent("mouseup", { button: 0, cancelable: true });
  window.dispatchEvent(other);
  expect(other.defaultPrevented).toBe(false);
});

it("leaves the item on show as it is when a pick from the history's list lands on it", async () => {
  const { issueRow, prRow } = await listing();
  for (const row of [issueRow, prRow, issueRow, prRow]) await fireEvent.click(row);
  await screen.findByRole("region", { name: "PR view" });
  /** Picks the second item of an arrow's list, two steps away. */
  const pick = async (way: "Back" | "Forward", title: string) => {
    const history = within(screen.getByRole("navigation", { name: "History" }));
    await fireEvent.contextMenu(history.getByRole("button", { name: way }));
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem");
    expect(items[1]?.textContent).toBe(title);
    await fireEvent.click(items[1] ?? document.body);
  };
  await vi.waitFor(() => {
    expect(screen.getByRole("navigation", { name: "History" }).textContent).toContain("#5 PR 5");
  });
  const readings = () => [core.openIssue.mock.calls.length, core.openPullRequest.mock.calls.length];
  const before = readings();

  await pick("Back", "#5 PR 5");
  expect(screen.getByRole("tab", { name: "Conversation" }).getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(prRow);
  await press("ArrowLeft", { altKey: true });
  await screen.findByRole("region", { name: "Issue view" });
  const back = readings();
  await pick("Forward", "#112564 GIL drop during collection");

  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  expect([before, readings()]).toEqual([before, back]);
});

it("leaves the mouse's own buttons to a dialog on top, and still keeps the page where it is", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn(), keysActive: false });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await unfoldAll();
  await fireEvent.click(
    screen.getByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  const released = new MouseEvent("mouseup", { button: 3, cancelable: true });
  window.dispatchEvent(released);

  expect(released.defaultPrevented).toBe(true);
  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
});

it("offers the palette Go Back and Go Forward, each with its key", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { component, issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  const command = (title: string) => component.paletteCommands().find((found) => found.title === title);

  expect([command("Go Back")?.note, command("Go Forward")?.note]).toEqual(["Alt+←", "Alt+→"]);
  command("Go Back")?.run();
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  command("Go Forward")?.run();
  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
});

it("moves the focus with the cursor on Alt+← and Alt+→", async () => {
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  treeRow.focus();

  await fireEvent.keyDown(treeRow, { key: "ArrowLeft", altKey: true });
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  expect(document.activeElement).toBe(issueRow);
  await fireEvent.keyDown(issueRow, { key: "ArrowRight", altKey: true });

  expect(await screen.findByRole("region", { name: "PR view" })).toBeTruthy();
  expect(document.activeElement).toBe(treeRow);
});

/** The PR view's item as the core reads it after the list did: newer activity, so another mark. */
const laterMark = { numbers: [160662], up_to: "2026-11-05T10:00:00Z" };

it("marks the line under the cursor on Q and Shift+Q, and the Detail pane's item when it holds the focus", async () => {
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, seen: { mark: laterMark, state: "new_activity" }, content: null },
  });
  const { issueRow, treeRow } = await listing();
  await fireEvent.click(treeRow);
  const view = await screen.findByRole("region", { name: "PR view" });
  await screen.findByRole("button", { name: "Mark Seen" });

  await fireEvent.keyDown(issueRow, { key: "q" });
  await fireEvent.keyDown(view, { key: "q" });
  await fireEvent.keyDown(issueRow, { key: "Q", shiftKey: true });
  await fireEvent.keyDown(issueRow, { key: "Q", shiftKey: true, repeat: true });

  expect(core.markSeen.mock.calls).toEqual([
    ["python/cpython", row.seen.mark],
    ["python/cpython", laterMark],
  ]);
  expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 160662);
});

/** A window on three Standalone PRs, #5, #6 and #8, the cursor on #5 and #5 on show. `markedSeen` answers
 * as the core does once it saved Mark Seen on #5, which sends it to the quiet section. */
const threeAlone = async () => {
  const send = catchChanged();
  const alone = (number: number, title: string) => ({
    ...standalone,
    number,
    title,
    attention: null,
    seen: { mark: { numbers: [number], up_to: "2026-11-04T10:00:00Z" }, state: "new_activity" },
  });
  const [first, next, last] = [alone(5, "Alone"), alone(6, "Next"), alone(8, "Last")];
  core.pullRequest.mockImplementation((_repository: string, number: number) =>
    Promise.resolve({
      status: "ok",
      data: { ...row, ...alone(number, `PR ${String(number)}`), content: null },
    }),
  );
  const answer = (items: object[], quiet: object[]) => ({
    status: "ok",
    data: { items, quiet, count: items.length, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  core.workbench.mockResolvedValue(answer([first, next, last], []));
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: () => {} });
  const marked = await screen.findByRole("button", { name: "#5 Alone" });
  await fireEvent.click(marked);
  const markedSeen = () => {
    core.workbench.mockResolvedValue(answer([next, last], [first]));
    send("python/cpython", [5]);
  };
  return { component, marked, markedSeen };
};

it("moves the cursor and the focus to the next row once Q sends the row under the cursor to the quiet section", async () => {
  const { marked, markedSeen } = await threeAlone();

  await fireEvent.keyDown(marked, { key: "q" });
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(landed);
  });
  expect(landed.getAttribute("aria-current")).toBe("true");
});

it("moves only the cursor to the next row once the palette's Mark Seen sends the item on show to the quiet section", async () => {
  const { component, markedSeen } = await threeAlone();
  await screen.findByRole("button", { name: "Mark Seen" });
  const before = document.activeElement;

  component
    .paletteCommands()
    .find(({ title }) => title === "Mark Seen")
    ?.run();
  await vi.waitFor(() => {
    expect(core.markSeen).toHaveBeenCalledOnce();
  });
  markedSeen();

  const landed = await screen.findByRole("button", { name: "#6 Next" });
  await vi.waitFor(() => {
    expect(landed.getAttribute("aria-current")).toBe("true");
  });
  await tick();
  expect(document.activeElement).toBe(before);
});

it("marks nothing on Q and Shift+Q with no line under the cursor, or with the list hidden and nothing open", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  await listing();

  await press("q");
  await press("Q", { shiftKey: true });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });
  await screen.findByText(/Nothing open/);
  await press("q");
  await press("Q", { shiftKey: true });

  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
});

it("offers Mark Unseen in the header of an item with nothing new, and the button marks it unseen", async () => {
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...row, number: 5, seen: { mark: row.seen.mark, state: "seen" }, content: null },
  });
  const { prRow } = await listing();
  await fireEvent.click(prRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mark Unseen" }));

  expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 5);
  expect(core.markSeen).not.toHaveBeenCalled();
});

it("lets Q and Shift+Q type in a text box", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const box = document.body.appendChild(document.createElement("input"));

  await fireEvent.keyDown(box, { key: "q" });
  await fireEvent.keyDown(box, { key: "Q", shiftKey: true });

  expect(core.markSeen).not.toHaveBeenCalled();
  expect(core.markUnseen).not.toHaveBeenCalled();
  box.remove();
});

it("offers the palette Mark Seen and Mark Unseen on the item on show, each with its key", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  const { component, issueRow } = await listing();
  const command = (title: string) => component.paletteCommands().find((found) => found.title === title);
  expect(command("Mark Seen")).toBeUndefined();
  await fireEvent.click(issueRow);
  await screen.findByRole("button", { name: "Mark Seen" });

  expect([command("Mark Seen")?.note, command("Mark Unseen")?.note]).toEqual(["Q", "Shift+Q"]);
  command("Mark Seen")?.run();
  command("Mark Unseen")?.run();

  await vi.waitFor(() => {
    expect(core.markUnseen).toHaveBeenCalledExactlyOnceWith("python/cpython", 112564);
  });
  expect(core.markSeen).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.seen.mark);
});

it("marks the item on show from its header's button, and says why the core refused", async () => {
  core.markSeen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "no database" } });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);

  await fireEvent.click(await screen.findByRole("button", { name: "Mark Seen" }));

  expect(core.markSeen).toHaveBeenCalledExactlyOnceWith("python/cpython", issue.seen.mark);
  expect((await screen.findByText(/couldn't mark/)).textContent).toBe(
    "Tsugiki couldn't mark this item: no database",
  );
});

it("unfolds the quiet section when Alt+← goes back to an item in it, and puts the cursor and focus there", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await fireEvent.click(screen.getByRole("button", { name: "Hide 1 quiet item" }));
  expect(screen.queryByRole("button", { name: "#5 Alone" })).toBeNull();

  await press("ArrowLeft", { altKey: true });

  const back = await screen.findByRole("button", { name: "#5 Alone" });
  expect(screen.getByRole("button", { name: "Hide 1 quiet item" })).toBeTruthy();
  expect(back.getAttribute("aria-current")).toBe("true");
  expect(document.activeElement).toBe(back);
});

it("folds and unfolds the quiet section from the palette, and folding moves the focus off a quiet row", async () => {
  const { component, issueRow, treeRow, prRow } = await listing();
  const command = (title: string) => component.paletteCommands().find((one) => one.title === title);
  await fireEvent.click(prRow);
  prRow.focus();

  command("Hide Quiet Items")?.run();

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(treeRow);
  });
  expect(treeRow.getAttribute("aria-current")).toBe("true");
  expect(screen.queryByRole("button", { name: "#5 Alone" })).toBeNull();
  expect(command("Hide Quiet Items")).toBeUndefined();
  issueRow.focus();
  command("Show Quiet Items")?.run();
  expect(await screen.findByRole("button", { name: "#5 Alone" })).toBeTruthy();
  command("Hide Quiet Items")?.run();
  await vi.waitFor(() => {
    expect(screen.queryByRole("button", { name: "#5 Alone" })).toBeNull();
  });
  expect(document.activeElement, "the focus wasn't on a row that went").toBe(issueRow);
});

it("gives the quiet line the focus when folding leaves no line above for the cursor", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [],
      quiet: [{ ...standalone, attention: null }],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await fireEvent.click(await screen.findByRole("button", { name: "Show 1 quiet item" }));
  const prRow = screen.getByRole("button", { name: "#5 Alone" });
  await fireEvent.click(prRow);
  prRow.focus();

  component
    .paletteCommands()
    .find((one) => one.title === "Hide Quiet Items")
    ?.run();

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Show 1 quiet item" }));
  });
});

it("offers no quiet command with nothing quiet", async () => {
  answering({ ...issue, attention: null });
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });

  expect(component.paletteCommands().map((command) => command.title)).not.toContain("Show Quiet Items");
});

it("shows another Repository as a launch does: its Workbench, an empty Detail pane, the cursor on top", async () => {
  const mine = { name: "Mine", query: "is:open author:@me", count: 1, problem: null };
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [],
      count: 1,
      review_requests: 0,
      stale_reviews: 0,
      filters: [mine],
    },
  });
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await fireEvent.click(await screen.findByRole("button", { name: "Mine 1" }));
  await unfoldAll();
  await fireEvent.click(
    await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  await rerender({ name: "python/other" });

  const workbench = await screen.findByRole("button", { name: "Workbench 1" });
  await vi.waitFor(() => {
    expect(workbench.getAttribute("aria-current")).toBe("true");
  });
  expect(core.workbench).toHaveBeenLastCalledWith("python/other", { kind: "whole" });
  expect(screen.queryByRole("region", { name: "PR view" })).toBeNull();
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
  // As at a launch, no line has the cursor yet, and the first ↓ puts it on the top one.
  const first = await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  expect(document.querySelector("[data-item][aria-current]")).toBeNull();
  await press("ArrowDown");
  expect(first.getAttribute("aria-current")).toBe("true");
});

it("unfolds an Issue with → and folds it with ←, which on a PR's row goes to its Issue first", async () => {
  const { issueRow, treeRow, prRow } = await listing();
  const fold = screen.getByRole("button", { name: "PRs of #112564" });
  const shown = () => fold.getAttribute("aria-expanded");
  await press("ArrowDown");
  await press("ArrowDown");
  expect(document.activeElement).toBe(treeRow);

  await press("ArrowRight");
  await press("ArrowLeft");
  const onIssue = [document.activeElement === issueRow, shown()];
  await press("ArrowLeft");
  const folded = shown();
  await press("ArrowDown");
  const skipped = document.activeElement === prRow;
  await press("ArrowLeft");
  await press("ArrowUp");
  await press("ArrowRight");

  expect([...onIssue, folded, skipped]).toEqual([true, "true", "false", true]);
  expect([shown(), document.activeElement]).toEqual(["true", issueRow]);
  await fireEvent.click(fold);
  expect(shown()).toBe("false");
});

it("leaves ← and → to whatever has the focus outside the list, such as a diff that scrolls sideways", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const pane = await screen.findByRole("region", { name: "Issue view" });

  const sideways = new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true });
  pane.dispatchEvent(sideways);

  expect(sideways.defaultPrevented).toBe(false);
  expect(screen.getByRole("button", { name: "PRs of #112564" }).getAttribute("aria-expanded")).toBe("true");
});

it("leaves ← and → to a pane's edge that has the focus, and to a hidden list", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const fold = screen.getByRole("button", { name: "PRs of #112564" });
  const edge = screen.getByRole("separator", { name: "Sidebar width" });

  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  const kept = fold.getAttribute("aria-expanded");
  await press("B", { ctrlKey: true, altKey: true });
  await press("ArrowLeft");
  await press("B", { ctrlKey: true, altKey: true });

  expect([
    kept,
    screen.getByRole("button", { name: "PRs of #112564" }).getAttribute("aria-expanded"),
  ]).toEqual(["true", "true"]);
});

it("goes back with Alt+← to a PR in a folded Issue, and unfolds it to put the cursor on the PR", async () => {
  const { treeRow, prRow } = await listing();
  await fireEvent.click(treeRow);
  await fireEvent.click(prRow);
  await fireEvent.click(screen.getByRole("button", { name: "PRs of #112564" }));

  await press("ArrowLeft", { altKey: true });

  const back = await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" });
  expect(back.getAttribute("aria-current")).toBe("true");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(back);
  });
});

it("starts the way back fresh in another Repository", async () => {
  answering({ ...issue, attention: null });
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  await rerender({ name: "python/other" });
  await unfoldAll();
  await fireEvent.click(
    await screen.findByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
  );
  await screen.findByRole("region", { name: "PR view" });

  await press("ArrowLeft", { altKey: true });

  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
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

it("leaves the browser's problem behind with the Repository it came from", async () => {
  core.openOnGithub.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  answering({ ...issue, attention: null });
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("button", { name: "#112564 GIL drop during collection" });
  await press("ArrowDown");
  await press("G", { ctrlKey: true, shiftKey: true });
  await screen.findByRole("alert");

  await rerender({ name: "python/other" });

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenLastCalledWith("python/other", { kind: "whole" });
  });
  expect(screen.queryByRole("alert")).toBeNull();
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
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
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

it("leaves F12 to the window's guard away from the Files tab, where it has nothing to go to", async () => {
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
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn(), keysActive: false });
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
    data: {
      items: [{ ...issue, attention: null }],
      quiet: [],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  const { rerender } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await fireEvent.click(await screen.findByRole("button", { name: "#112564 GIL drop during collection" }));
  expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();

  await rerender({ name: "python/other" });

  expect(screen.queryByRole("region", { name: "Issue view" })).toBeNull();
});

/** Lays the panes out `sidebar`, `list` and `detail` pixels wide, as jsdom lays nothing out. */
const laidOut = (sidebar: number, list: number, detail: number) => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const width = this.matches("nav") ? sidebar : this.matches(".list") ? list : detail;
    return { width } as DOMRect;
  });
  // jsdom has no pointer capture.
  HTMLElement.prototype.setPointerCapture = vi.fn();
};

const home = () => document.querySelector<HTMLElement>(".home") as HTMLElement;

// The panes read which of them holds the focus with :focus-within, which e2e/focus.spec.ts checks in a
// browser. Only whether the window holds it needs the page's help.
it("marks the panes as out of focus while another app is in front", async () => {
  await listing();
  const marked = () => home().classList.contains("window-focused");

  await fireEvent.blur(window);
  const away = marked();
  await fireEvent.focus(window);

  expect([away, marked()]).toEqual([false, true]);
  expect(document.querySelector('[data-pane="list"]')?.classList.contains("list")).toBe(true);
});

it("drags the Sidebar's edge between its minimum and maximum, saves where it stops, and resets on a double-click", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  // A click that doesn't move the edge has nothing to save.
  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerUp(edge);

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 340 });
  const dragged = [home().style.getPropertyValue("--sidebar-width"), edge.getAttribute("aria-valuenow")];
  await fireEvent.pointerMove(edge, { clientX: 2000 });
  const widest = home().style.getPropertyValue("--sidebar-width");
  await fireEvent.pointerMove(edge, { clientX: 0 });
  await fireEvent.pointerUp(edge);
  // After the drag, the pointer moves the edge no more, and the other button never does.
  await fireEvent.pointerMove(edge, { clientX: 340 });
  await fireEvent.pointerUp(edge);
  await fireEvent.pointerDown(edge, { button: 2, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 340 });
  const narrowest = home().style.getPropertyValue("--sidebar-width");

  await fireEvent.dblClick(edge);

  expect([dragged, widest, narrowest]).toEqual([["240px", "240"], "480px", "140px"]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([[{ ...DEFAULT_LAYOUT, sidebar: 140 }], [DEFAULT_LAYOUT]]);
  });
  expect(home().style.getPropertyValue("--sidebar-width")).toBe("200px");
});

it("stops the Sidebar's edge where the list would go under its minimum", async () => {
  laidOut(200, 300, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 400 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("220px");
});

it("stops the Sidebar's edge where the Detail pane would go under its minimum while the list is hidden", async () => {
  laidOut(200, 600, 500);
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await toggleList();
  const edge = screen.getByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 300, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 2000 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("380px");
});

it("offers the list's edge beside a Detail pane, and keeps the Detail pane's minimum", async () => {
  laidOut(200, 600, 500);
  const { issueRow } = await listing();
  const before = screen.queryByRole("separator", { name: "Workbench width" });
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const edge = screen.getByRole("separator", { name: "Workbench width" });
  const list = document.querySelector(".list") as HTMLElement;
  const even = [list.classList.contains("sized"), home().style.getPropertyValue("--list-width")];

  await fireEvent.pointerDown(edge, { button: 0, clientX: 800, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 2000 });
  const widest = home().style.getPropertyValue("--list-width");
  await fireEvent.pointerMove(edge, { clientX: 0 });
  await fireEvent.pointerUp(edge);

  expect([before, even, widest]).toEqual([null, [false, ""], "780px"]);
  expect([home().style.getPropertyValue("--list-width"), list.classList.contains("sized")]).toEqual([
    "280px",
    true,
  ]);
  expect(core.savePaneLayout).toHaveBeenCalledExactlyOnceWith({ ...DEFAULT_LAYOUT, list: 280 });
  await fireEvent.dblClick(edge);
  expect(list.classList.contains("sized")).toBe(false);
});

it("lets the Sidebar grow into the room the Detail pane has to spare", async () => {
  laidOut(200, 280, 700);
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  const edge = screen.getByRole("separator", { name: "Sidebar width" });

  await fireEvent.pointerDown(edge, { button: 0, clientX: 200, pointerId: 1 });
  await fireEvent.pointerMove(edge, { clientX: 400 });

  expect(home().style.getPropertyValue("--sidebar-width")).toBe("400px");
});

it("moves an edge with the bare arrow keys, and saves a held key once, as it comes up", async () => {
  laidOut(200, 600, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });

  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  const right = [home().style.getPropertyValue("--sidebar-width"), core.savePaneLayout.mock.calls.length];
  await fireEvent.keyUp(edge, { key: "ArrowRight" });
  // The layout jsdom stands in for still says 200 px, so both presses land on 184 px.
  await fireEvent.keyDown(edge, { key: "ArrowLeft" });
  await fireEvent.keyDown(edge, { key: "ArrowLeft", repeat: true });
  await fireEvent.keyUp(edge, { key: "ArrowLeft" });
  await fireEvent.keyDown(edge, { key: "ArrowLeft", altKey: true });
  await fireEvent.keyDown(edge, { key: "Enter" });
  await fireEvent.keyUp(edge, { key: "Enter" });

  expect([right, home().style.getPropertyValue("--sidebar-width")]).toEqual([["216px", 0], "184px"]);
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 216 }],
      [{ ...DEFAULT_LAYOUT, sidebar: 184 }],
    ]);
  });
});

it("opens with the widths saved on this machine, and says when it can't save them", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260, list: 500 });
  core.savePaneLayout.mockResolvedValue({ status: "error", error: { kind: "internal", message: "locked" } });
  laidOut(260, 500, 0);
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("260");
  });

  await fireEvent.keyDown(edge, { key: "ArrowRight" });
  await fireEvent.keyUp(edge, { key: "ArrowRight" });

  expect(home().style.getPropertyValue("--list-width")).toBe("500px");
  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't save the panes' layout: locked");
});

it("hides the Sidebar and its edge with Ctrl+B, and brings them back at their width", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260 });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  const edge = await screen.findByRole("separator", { name: "Sidebar width" });
  await vi.waitFor(() => {
    expect(edge.getAttribute("aria-valuenow")).toBe("260");
  });

  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  const hidden = [screen.queryByRole("navigation"), screen.queryByRole("separator")];
  // A held key flips it no more.
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true, repeat: true });
  const held = screen.queryByRole("navigation");
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });

  expect([hidden, held]).toEqual([[null, null], null]);
  expect(screen.getByRole("separator", { name: "Sidebar width" }).getAttribute("aria-valuenow")).toBe("260");
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, sidebar: 260, sidebar_hidden: true }],
      [{ ...DEFAULT_LAYOUT, sidebar: 260 }],
    ]);
  });
});

it("opens with the Sidebar hidden when it was hidden on this machine", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });

  await vi.waitFor(() => {
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});

it("leaves the Sidebar alone on Ctrl+B while a dialog is on top", async () => {
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn(), keysActive: false });
  await screen.findByRole("navigation", { name: "Sidebar" });

  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });

  expect(screen.getByRole("navigation", { name: "Sidebar" })).toBeTruthy();
  expect(core.savePaneLayout).not.toHaveBeenCalled();
});

it("offers the palette a command that hides or shows the Sidebar, named for what it does", async () => {
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const [hide] = component.paletteCommands();

  hide?.run();
  await vi.waitFor(() => {
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  expect([hide?.title, component.paletteCommands()[0]?.title]).toEqual(["Hide Sidebar", "Show Sidebar"]);
});

it("moves the focus to the cursor's row when Ctrl+B hides the Sidebar it was in", async () => {
  const { issueRow } = await listing();
  await fireEvent.keyDown(window, { key: "ArrowDown" });
  const view = screen.getByRole("button", { name: /^Review requests/ });
  view.focus();

  await fireEvent.keyDown(view, { key: "b", code: "KeyB", ctrlKey: true });

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(issueRow);
  });
});

/** Presses Ctrl+Alt+B, which hides the Issue list or brings it back. */
const toggleList = (target: Element = document.body, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(target, { key: "b", code: "KeyB", ctrlKey: true, altKey: true, ...init });

const repositoryName = () => screen.queryByRole("heading", { name: "python/cpython" });

/** The list's row named `name`, which the Detail pane may name too. */
const rowNamed = (name: string) =>
  within(document.querySelector<HTMLElement>(".list") as HTMLElement).getByRole("button", { name });

it("hides the Issue list and its edge with Ctrl+Alt+B, and brings them back at their width", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, list: 500 });
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("separator", { name: "Workbench width" });

  await toggleList();
  const hidden = [repositoryName(), screen.queryByRole("separator", { name: "Workbench width" })];
  const detail = screen.queryByRole("region", { name: "Issue view" });
  // A held key flips it no more.
  await toggleList(document.body, { repeat: true });
  const held = repositoryName();
  await toggleList();

  expect([hidden, held, detail === null]).toEqual([[null, null], null, false]);
  expect(screen.getByRole("separator", { name: "Workbench width" })).toBeTruthy();
  expect(home().style.getPropertyValue("--list-width")).toBe("500px");
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.calls).toEqual([
      [{ ...DEFAULT_LAYOUT, list: 500, list_hidden: true }],
      [{ ...DEFAULT_LAYOUT, list: 500 }],
    ]);
  });
});

it("opens with the Issue list hidden when it was hidden on this machine", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, list_hidden: true });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });

  await vi.waitFor(() => {
    expect(repositoryName()).toBeNull();
  });
});

it("leaves the Issue list alone on Ctrl+Alt+B while a dialog is on top, and in a text box", async () => {
  const { rerender } = render(RepositoryHome, {
    name: "python/cpython",
    onpick: () => {},
    onadd: vi.fn(),
    keysActive: false,
  });
  await vi.waitFor(() => {
    expect(core.paneLayout).toHaveBeenCalled();
  });
  await toggleList();
  const behindDialog = repositoryName();
  await rerender({ keysActive: true });
  const box = document.createElement("input");
  document.body.append(box);

  await toggleList(box);
  box.remove();

  expect([behindDialog === null, repositoryName() === null]).toEqual([false, false]);
  expect(core.savePaneLayout).not.toHaveBeenCalled();
});

it("offers the palette a command that hides or shows the Issue list, named for what it does", async () => {
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const titles = () => component.paletteCommands().map((command) => command.title);
  const before = titles();

  component
    .paletteCommands()
    .find((command) => command.title === "Hide Issue List")
    ?.run();
  await vi.waitFor(() => {
    expect(repositoryName()).toBeNull();
  });

  expect([before, titles()]).toEqual([
    [
      "Hide Sidebar",
      "Hide Issue List",
      "Show Only the Detail Pane",
      "Reset Layout",
      "Go Back",
      "Go Forward",
      "Sort by Last Update",
      "View: Stale Reviews",
    ],
    [
      "Hide Sidebar",
      "Show Issue List",
      "Show Only the Detail Pane",
      "Reset Layout",
      "Go Back",
      "Go Forward",
      "Sort by Last Update",
      "View: Stale Reviews",
    ],
  ]);
});

it("offers the palette Stale reviews, which shows the list with that view", async () => {
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const run = (title: string) => {
    component
      .paletteCommands()
      .find((command) => command.title === title)
      ?.run();
  };
  run("Hide Issue List");
  await vi.waitFor(() => {
    expect(repositoryName()).toBeNull();
  });

  run("View: Stale Reviews");

  expect(await screen.findByRole("heading", { name: "Stale reviews" })).toBeTruthy();
  expect(repositoryName()).not.toBeNull();
  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "stale_reviews" });
  expect(screen.getByRole("button", { name: /^Stale reviews/ }).getAttribute("aria-current")).toBe("true");
});

it("names the view over the list, and switches its order, which the list asks for once it's saved", async () => {
  const { component } = await listing();
  expect(screen.getByRole("heading", { name: "Workbench" })).toBeTruthy();
  const asked = core.workbench.mock.calls.length;

  await fireEvent.click(screen.getByRole("button", { name: "By attention ⇅" }));

  await vi.waitFor(() => {
    expect(core.workbench).toHaveBeenCalledTimes(asked + 1);
  });
  expect(core.savePaneLayout).toHaveBeenCalledExactlyOnceWith({ ...DEFAULT_LAYOUT, order: "last_update" });
  expect(core.savePaneLayout.mock.invocationCallOrder[0]).toBeLessThan(
    core.workbench.mock.invocationCallOrder[asked] ?? 0,
  );
  await fireEvent.click(screen.getByRole("button", { name: "By last update ⇅" }));
  expect(await screen.findByRole("button", { name: "By attention ⇅" })).toBeTruthy();
  expect(core.savePaneLayout).toHaveBeenLastCalledWith(DEFAULT_LAYOUT);

  // The palette offers the order the list isn't in.
  const sort = () => component.paletteCommands().find((command) => command.title.startsWith("Sort by"));
  const offered = [sort()?.title];
  sort()?.run();
  await screen.findByRole("button", { name: "By last update ⇅" });
  offered.push(sort()?.title);
  sort()?.run();
  expect(await screen.findByRole("button", { name: "By attention ⇅" })).toBeTruthy();
  expect(offered).toEqual(["Sort by Last Update", "Sort by Attention"]);
});

/** Presses Ctrl+K Z, which the page hands the screen once the chord ends. */
const detailOnly = async (component: { chordBindings: () => ChordBindings }) => {
  component.chordBindings()["ctrl+k z"]?.();
  await tick();
};

const sidebar = () => screen.queryByRole("navigation", { name: "Sidebar" });

it("shows only the Detail pane on Ctrl+K Z, with the focus in it, and brings back both panes", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");

  await detailOnly(component);
  const hidden = [sidebar(), repositoryName()];
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
  await detailOnly(component);

  expect(hidden).toEqual([null, null]);
  expect([sidebar() === null, repositoryName() === null]).toEqual([false, false]);
  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
  expect(row.getAttribute("aria-current")).toBe("true");
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULT_LAYOUT, sidebar_hidden: true, list_hidden: true, detail_only_hid: "both" }],
    [DEFAULT_LAYOUT],
  ]);
});

it("brings back only the Sidebar on Ctrl+K Z when the list was hidden, with the focus left in the Detail pane", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  await toggleList();

  await detailOnly(component);
  const hidden = sidebar();
  await detailOnly(component);

  expect([hidden, sidebar() === null, repositoryName()]).toEqual([null, false, null]);
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("leaves the focus on a control in the Detail pane while Ctrl+K Z keeps the list hidden", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  await toggleList();
  const title = await within(view).findByRole("button", { name: "#112564 GIL drop during collection" });
  title.focus();

  await detailOnly(component);
  await tick();
  const hiding = document.activeElement;
  await detailOnly(component);
  await tick();

  expect([hiding, document.activeElement]).toEqual([title, title]);
});

it("offers the palette Ctrl+K Z's command, named for what it does", async () => {
  const { component } = render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("navigation", { name: "Sidebar" });
  const command = (title: string) => component.paletteCommands().find((one) => one.title === title);

  command("Show Only the Detail Pane")?.run();
  await vi.waitFor(() => {
    expect(sidebar()).toBeNull();
  });
  const hidden = repositoryName();
  command("Show the Panes")?.run();

  await vi.waitFor(() => {
    expect(sidebar()).toBeTruthy();
  });
  expect([hidden, repositoryName() === null]).toEqual([null, false]);
});

/** Runs the palette's "Reset Layout". */
const resetLayout = (component: { paletteCommands: () => { title: string; run: () => void }[] }) => {
  const command = component.paletteCommands().find(({ title }) => title === "Reset Layout");
  if (!command) throw new Error("The palette offers no Reset Layout");
  command.run();
};

it("resets the layout from the palette: both panes on show at their default widths, saved", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULT_LAYOUT, sidebar: 260, list: 500 });
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await fireEvent.keyDown(window, { key: "b", code: "KeyB", ctrlKey: true });
  await toggleList();

  resetLayout(component);

  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.lastCall).toEqual([DEFAULT_LAYOUT]);
  });
  expect(screen.getByRole("navigation", { name: "Sidebar" })).toBeTruthy();
  expect(repositoryName()).toBeTruthy();
  expect(home().style.getPropertyValue("--sidebar-width")).toBe("200px");
  expect(home().style.getPropertyValue("--list-width")).toBe("");
});

it("brings the list back on Reset Layout with the cursor and the focus on the open item's row", async () => {
  const { component, issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await press("ArrowDown");
  await toggleList();

  resetLayout(component);

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(rowNamed("#112564 GIL drop during collection"));
  });
  expect(document.activeElement?.getAttribute("aria-current")).toBe("true");
});

it("leaves the cursor and the focus alone on Reset Layout while the list shows", async () => {
  const { component, issueRow, treeRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");

  resetLayout(component);
  await vi.waitFor(() => {
    expect(core.savePaneLayout).toHaveBeenCalled();
  });
  await tick();

  expect(document.activeElement).toBe(treeRow);
  expect(treeRow.getAttribute("aria-current")).toBe("true");
});

it("moves the focus to the Detail pane as Ctrl+Alt+B hides the list", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  issueRow.focus();

  await toggleList(issueRow);

  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("brings the list back with the cursor and the focus on the open item's row", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  // The cursor leaves the Issue on show for the PR under it.
  await press("ArrowDown");
  await toggleList();

  await toggleList();

  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
  expect(row.getAttribute("aria-current")).toBe("true");
});

it("brings the list back with the focus on the cursor's row when the open item isn't in it", async () => {
  const { treeRow } = await listing();
  await fireEvent.click(treeRow);
  await screen.findByRole("region", { name: "PR view" });
  await press("ArrowUp");
  // The PR leaves the Issue's tree, so the list has no row for what the Detail pane shows.
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [{ ...issue, pull_requests: [], attention: null }],
      quiet: [],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Review requests 0" }));
  await vi.waitFor(() => {
    expect(
      screen.queryByRole("button", { name: "#160662 gh-112564: Fix UAF for gc debug print" }),
    ).toBeNull();
  });
  await toggleList();

  await toggleList();

  const row = rowNamed("#112564 GIL drop during collection");
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(row);
  });
});

it("scrolls the Detail pane with ↑ and ↓ while it holds the focus beside the list (#602)", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const scrollBy = vi.fn();
  Object.defineProperty(view, "scrollBy", { value: scrollBy });
  const title = within(view).getByRole("button", { name: "#112564 GIL drop during collection" });

  await fireEvent.keyDown(title, { key: "ArrowDown" });
  await fireEvent.keyDown(title, { key: "ArrowUp" });
  const cursor = rowNamed("#112564 GIL drop during collection").getAttribute("aria-current");
  await press("ArrowDown");

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }], [{ top: -40 }]]);
  expect(cursor, "the list's cursor stays while the Detail pane scrolls").toBe("true");
  expect(rowNamed("#160662 gh-112564: Fix UAF for gc debug print").getAttribute("aria-current")).toBe("true");
});

it("scrolls the Detail pane with ↑ and ↓ while the list is hidden, and leaves the cursor alone", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const scrollBy = vi.fn();
  Object.defineProperty(view, "scrollBy", { value: scrollBy });
  await toggleList();

  await press("ArrowDown");
  await press("ArrowUp");
  await toggleList();

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }], [{ top: -40 }]]);
  expect(rowNamed("#112564 GIL drop during collection").getAttribute("aria-current")).toBe("true");
});

it("lets the Detail pane take the focus only while the list is hidden, so a click in it leaves Enter alone", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  const view = await screen.findByRole("region", { name: "Issue view" });
  const shown = view.getAttribute("tabindex");

  await toggleList();

  expect([shown, view.getAttribute("tabindex")]).toEqual([null, "-1"]);
});

it("says what went wrong above the panes while the list is hidden", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "error", error: { kind: "internal", message: "locked" } });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await screen.findByRole("navigation", { name: "Sidebar" });

  await toggleList();

  expect((await screen.findByRole("alert")).textContent).toBe("Tsugiki can't save the panes' layout: locked");
  expect(repositoryName()).toBeNull();
});

it("opens nothing on GitHub with Ctrl+Shift+G while the list is hidden and nothing is open", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();

  await press("G", { ctrlKey: true, shiftKey: true });

  expect(core.openOnGithub).not.toHaveBeenCalled();
});

it("opens nothing on Enter while the list is hidden, as the cursor's row doesn't show", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();

  await press("Enter");

  expect(screen.getByRole("region", { name: "Detail pane" })).toBeTruthy();
  expect(core.openIssue).not.toHaveBeenCalled();
});

it("scrolls the file view's code with ↑ and ↓ while the list is hidden, and not the diff under it", async () => {
  await onFileView();
  const diff = await screen.findByRole("region", { name: "Diff", hidden: true });
  const code = screen.getByRole("region", { name: "Definition's code" });
  const [onDiff, onCode] = [vi.fn(), vi.fn()];
  Object.defineProperty(diff, "scrollBy", { value: onDiff });
  Object.defineProperty(code, "scrollBy", { value: onCode });
  await toggleList();

  await press("ArrowDown");

  expect(onCode.mock.calls).toEqual([[{ top: 40 }]]);
  expect(onDiff).not.toHaveBeenCalled();
});

it("scrolls the diff with ↑ and ↓ on a PR's Files tab while the list is hidden", async () => {
  const files = await onFiles();
  await fireEvent.click(files);
  const diff = await screen.findByRole("region", { name: "Diff" });
  const scrollBy = vi.fn();
  Object.defineProperty(diff, "scrollBy", { value: scrollBy });
  await toggleList();

  await press("ArrowDown");

  expect(scrollBy.mock.calls).toEqual([[{ top: 40 }]]);
});

it("still goes back through the Issues and PRs it opened with Alt+← while the list is hidden", async () => {
  const { issueRow, prRow } = await listing();
  await fireEvent.click(issueRow);
  await fireEvent.click(prRow);
  await screen.findByRole("region", { name: "PR view" });
  await toggleList();

  await press("ArrowLeft", { altKey: true });

  const view = await screen.findByRole("region", { name: "Issue view" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(view);
  });
});

it("brings the list back with the view a click on the Sidebar picks, and keeps the Detail pane", async () => {
  const { issueRow } = await listing();
  await fireEvent.click(issueRow);
  await screen.findByRole("region", { name: "Issue view" });
  await toggleList();

  await fireEvent.click(screen.getByRole("button", { name: "Review requests 0" }));

  expect(core.workbench).toHaveBeenLastCalledWith("python/cpython", { kind: "review_requests" });
  expect(repositoryName()).toBeTruthy();
  expect(screen.getByRole("region", { name: "Issue view" })).toBeTruthy();
  await vi.waitFor(() => {
    expect(core.savePaneLayout.mock.lastCall).toEqual([DEFAULT_LAYOUT]);
  });
});

it("says how to bring the list back when nothing is open, and does so on a click of the key", async () => {
  await listing();
  await press("ArrowDown");
  await toggleList();
  const empty = screen.getByRole("region", { name: "Detail pane" });
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(empty);
  });
  const said = empty.textContent;

  await fireEvent.click(within(empty).getByRole("button", { name: "Ctrl+Alt+B" }));

  expect(said).toBe("Nothing open. Ctrl+Alt+B shows the Issue list.");
  expect(repositoryName()).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Detail pane" })).toBeNull();
  await vi.waitFor(() => {
    expect(document.activeElement).toBe(rowNamed("#112564 GIL drop during collection"));
  });
});

it("marks each Issue and PR in the list with the icon of its state", async () => {
  core.workbench.mockResolvedValue({
    status: "ok",
    data: {
      items: [
        { ...issue, state: "NOT_PLANNED", attention: null },
        { ...issue, number: 1, state: null, pull_requests: [], attention: null },
      ],
      quiet: [{ ...standalone, is_draft: true, attention: null }],
      count: 0,
      review_requests: 0,
      stale_reviews: 0,
      filters: [],
    },
  });
  render(RepositoryHome, { name: "python/cpython", onpick: () => {}, onadd: vi.fn() });
  await unfoldAll();
  await fireEvent.click(screen.getByRole("button", { name: "Show 1 quiet item" }));

  const icons = screen.getAllByRole("img").map((icon) => icon.getAttribute("title"));

  expect(icons).toEqual(["Closed as not planned", "Open PR", "Draft PR"]);
});
