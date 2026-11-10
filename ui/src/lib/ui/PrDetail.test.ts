import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PrViewState, type PrView } from "$lib/state/pr-view.svelte";
import PrDetail from "./PrDetail.svelte";

const core = vi.hoisted(() => ({
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  fetchPullRequest: vi.fn(),
  openOnGithub: vi.fn(),
  openLink: vi.fn(),
  pullRequestFiles: vi.fn(),
  cloning: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequest: core.pullRequest,
    openPullRequest: core.openPullRequest,
    fetchPullRequest: core.fetchPullRequest,
    openOnGithub: core.openOnGithub,
    openLink: core.openLink,
    pullRequestFiles: core.pullRequestFiles,
    cloning: core.cloning,
  },
  events: {
    changed: { listen: core.listen },
    readFailed: { listen: core.listen },
    headsFetched: { listen: core.listen },
    headsFetchFailed: { listen: core.listen },
    cloningChanged: { listen: core.listen },
    mirrorStatesChanged: { listen: core.listen },
  },
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
        hidden: null,
      },
    ],
    earlier_comments: 2,
    reviews: [
      {
        author: "some-reviewer",
        state: "APPROVED",
        commit: "0123abcdef",
        submitted_at: "2026-11-04T10:00:00Z",
        body_html: "",
        hidden: null,
        threads: [],
      },
    ],
    earlier_reviews: 0,
  },
};

const NOW = new Date("2026-11-04T11:00:00Z");

/** The PR on show, which stops after each test, as its Files tab loads once it shows. */
let onShow: PrViewState | undefined;

/** Shows the PR as opened from Issue `issue`. */
const shown = async (answer: PrView | null, issue: number | undefined, onback = vi.fn()) => {
  core.pullRequest.mockResolvedValue({ status: "ok", data: answer });
  const state = new PrViewState("python/cpython", 11, issue);
  onShow = state;
  await state.load();
  render(PrDetail, { repository: "python/cpython", pr: state, now: NOW, onback });
  return state;
};

afterEach(() => {
  onShow?.stop();
  onShow = undefined;
});

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(vi.fn());
  core.openPullRequest.mockResolvedValue({ status: "ok", data: null });
  core.openOnGithub.mockResolvedValue({ status: "ok", data: null });
  core.openLink.mockResolvedValue({ status: "ok", data: null });
  core.pullRequestFiles.mockResolvedValue({ status: "ok", data: { state: "fetching" } });
  core.cloning.mockResolvedValue(null);
});

it("opens on the Conversation: the title and author, the description, comments and Reviews", async () => {
  await shown(view, 10);

  expect(screen.getByRole("region", { name: "PR view" })).toBeTruthy();
  expect(screen.getByRole("heading").textContent).toContain("#11 gh-10: Fix the crash");
  expect(screen.getByText("pr-author")).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Conversation" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("false");
  expect(screen.getByText("The description").tagName).toBe("P");
  expect(screen.getByText("some-reviewer · 2h ago")).toBeTruthy();
  expect(screen.getByText("2 earlier comments are on GitHub.")).toBeTruthy();
  expect(screen.getByText("some-reviewer approved at 0123abc · 1h ago")).toBeTruthy();
});

it("marks the PR's title with the icon of its state", async () => {
  await shown({ ...view, state: "OPEN", is_draft: true }, 10);

  expect(within(screen.getByRole("heading")).getByRole("img").getAttribute("title")).toBe("Draft PR");
});

it("says it's reading from GitHub until the first read lands", async () => {
  await shown({ ...view, author: null, content: null }, 10);

  expect(screen.getByText("Reading from GitHub…")).toBeTruthy();
  expect(screen.getByText("deleted user")).toBeTruthy();
});

it.each([
  [
    "no_token",
    "Tsugiki can't ask GitHub for the Conversation without a token. The status bar says how to sign in.",
  ],
  ["waits_on_github", "The Conversation comes once GitHub answers."],
  ["not_on_github", "GitHub doesn't have this PR, or your token can't see it."],
  ["other", "Tsugiki couldn't read this PR's Conversation from GitHub. tsugiki.log says why."],
] as const)("says why the first read failed: %s", async (why, words) => {
  const state = await shown({ ...view, content: null }, 10);

  state.failedRead = why;

  await vi.waitFor(() => expect(screen.getByText(words)).toBeTruthy());
  expect(screen.queryByText("Reading from GitHub…")).toBeNull();
});

it("shows the Conversation it holds when a later read fails", async () => {
  const state = await shown(view, 10);

  state.failedRead = "waits_on_github";

  await vi.waitFor(() => expect(screen.getByText("The description")).toBeTruthy());
  expect(screen.queryByText("The Conversation comes once GitHub answers.")).toBeNull();
});

it("switches to the Files tab on a click, which waits for the PR's commits", async () => {
  const state = await shown(view, 10);

  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  expect(state.tab).toBe("files");
  expect(screen.getByRole("tab", { name: "Files" }).getAttribute("aria-selected")).toBe("true");
  await vi.waitFor(() =>
    expect(screen.getByRole("tabpanel", { name: "Files" }).textContent?.trim()).toBe(
      "Fetching the PR's commits…",
    ),
  );
  expect(screen.queryByText("The description")).toBeNull();

  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
  expect(state.tab).toBe("conversation");
});

it("opens a thread's place in the Files tab", async () => {
  const thread = {
    id: "T1",
    path: "Lib/ast.py",
    subject: { kind: "line" as const, side: "RIGHT" as const, line: 12, original: 12 },
    opening: { kind: "words" as const, words: "Why not a set?" },
    replies: 0,
    resolution: "open" as const,
    anchor: "current" as const,
  };
  const reviews = view.content?.reviews.map((review) => ({ ...review, threads: [thread] })) ?? [];
  const content = view.content === null ? null : { ...view.content, reviews };
  const state = await shown({ ...view, content }, 10);
  const reveal = vi.spyOn(state.files, "reveal");

  await fireEvent.click(screen.getByRole("button", { name: "Lib/ast.py:12" }));

  expect(state.tab).toBe("files");
  expect(reveal).toHaveBeenCalledExactlyOnceWith("T1");
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

  expect(onback).toHaveBeenCalledExactlyOnceWith(10);
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

it("sends a link in a NEWS entry to the browser", async () => {
  // jsdom lays nothing out, so bind:clientHeight has nothing to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const url = "https://github.com/python/cpython/issues/1";
  const news = [
    {
      path: "Misc/NEWS.d/next/Library/a.rst",
      section: "Library",
      issue: { text: "gh-1", to: { site: "gh_issue" as const, number: 1 } },
      blocks: [],
    },
  ];
  core.pullRequestFiles.mockResolvedValue({
    status: "ok",
    data: { state: "ready", head: "abc", base: "base", files: [], rows: 0, news, threads: [] },
  });
  await shown(view, 10);
  await fireEvent.click(screen.getByRole("tab", { name: "Files" }));

  await fireEvent.click(await screen.findByRole("link", { name: "gh-1" }));

  expect(core.openLink).toHaveBeenCalledExactlyOnceWith(url);
  vi.unstubAllGlobals();
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
