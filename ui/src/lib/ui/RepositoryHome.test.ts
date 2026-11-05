import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import RepositoryHome from "./RepositoryHome.svelte";

const core = vi.hoisted(() => ({
  workbench: vi.fn(),
  listen: vi.fn(),
  stopListening: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { workbench: core.workbench },
  events: { changed: { listen: core.listen } },
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
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
});

afterEach(() => {
  vi.useRealTimers();
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
