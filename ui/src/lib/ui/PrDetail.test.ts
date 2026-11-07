import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { PrViewState, type PrView } from "$lib/state/pr-view.svelte";
import PrDetail from "./PrDetail.svelte";

const core = vi.hoisted(() => ({
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  fetchPullRequest: vi.fn(),
  openOnGithub: vi.fn(),
  openLink: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequest: core.pullRequest,
    openPullRequest: core.openPullRequest,
    fetchPullRequest: core.fetchPullRequest,
    openOnGithub: core.openOnGithub,
    openLink: core.openLink,
  },
  events: { changed: { listen: core.listen } },
}));

const view: PrView = {
  number: 11,
  title: "gh-10: Fix the crash",
  author: "pr-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: null,
  content: {
    body_html: "<p>The description</p>",
    comments: [
      {
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: '<p><a href="https://github.com/python/cpython/pull/1">A link</a></p>',
      },
    ],
    earlier_comments: 2,
  },
};

const NOW = new Date("2026-11-04T11:00:00Z");

/** Shows the PR as opened from Issue `issue`. */
const shown = async (answer: PrView | null, issue: number | undefined, onback = vi.fn()) => {
  core.pullRequest.mockResolvedValue({ status: "ok", data: answer });
  const state = new PrViewState("python/cpython", 11, issue);
  await state.load();
  render(PrDetail, { repository: "python/cpython", pr: state, now: NOW, onback });
  state.stop();
  return state;
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(vi.fn());
  core.openPullRequest.mockResolvedValue({ status: "ok", data: null });
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.openLink.mockResolvedValue({ status: "ok", data: null });
});

it("opens on the Conversation: the title and author, the description and the recent comments", async () => {
  await shown(view, 10);

  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(screen.getByRole("heading").textContent).toContain("#11 gh-10: Fix the crash");
  expect(screen.getByText("pr-author")).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Conversation" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("false");
  expect(screen.getByText("The description").tagName).toBe("P");
  expect(screen.getByText("some-reviewer · 2h ago")).toBeTruthy();
  expect(screen.getByText("2 earlier comments are on GitHub.")).toBeTruthy();
});

it("says it's reading from GitHub until the first read lands", async () => {
  await shown({ ...view, author: null, content: null }, 10);

  expect(screen.getByText("Reading from GitHub…")).toBeTruthy();
  expect(screen.getByText("deleted user")).toBeTruthy();
});

it("switches to the Files tab on a click, which holds nothing yet", async () => {
  const state = await shown(view, 10);

  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  expect(state.tab).toBe("files");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tabpanel", { name: "Files" }).textContent?.trim()).toBe("");
  expect(screen.queryByText("The description")).toBeNull();

  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
  expect(state.tab).toBe("conversation");
});

it("leaves a click on the tab already open alone", async () => {
  const state = await shown(view, 10);

  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));

  expect(state.tab).toBe("conversation");
});

it("goes back to the Issue with its gh-N link", async () => {
  const onback = vi.fn();
  await shown(view, 10, onback);

  await fireEvent.click(screen.getByRole("button", { name: "gh-10" }));

  expect(onback).toHaveBeenCalledOnce();
});

it("has no way back for a PR opened from no Issue", async () => {
  await shown(view, undefined);

  expect(screen.queryByRole("button", { name: /^gh-/ })).toBeNull();
});

it("opens the title on GitHub", async () => {
  await shown(view, 10);

  await fireEvent.click(screen.getByRole("button", { name: "#11 gh-10: Fix the crash" }));

  expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("python/cpython", {
    kind: "pull_request",
    number: 11,
  });
});

it("sends a link in a comment to the browser, and says if that fails", async () => {
  core.openLink.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki couldn't open the system browser." },
  });
  await shown(view, 10);

  await fireEvent.click(screen.getByRole("link", { name: "A link" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/pull/1");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Tsugiki couldn't open the system browser.",
  );
});

it("says so for a PR Tsugiki doesn't hold", async () => {
  await shown(null, 10);

  expect(screen.getByText("Tsugiki doesn't hold PR #11.")).toBeTruthy();
});

it("says why it can't show the PR", async () => {
  core.pullRequest.mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "no database" },
  });
  const state = new PrViewState("python/cpython", 11, 10);
  await state.load();
  render(PrDetail, { repository: "python/cpython", pr: state, now: NOW, onback: vi.fn() });
  state.stop();

  expect(screen.getByRole("alert").textContent).toContain("Tsugiki can't show this PR: no database");
});
