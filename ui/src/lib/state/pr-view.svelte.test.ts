import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { PrViewState, type PrView } from "./pr-view.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequest: vi.fn(),
    openPullRequest: vi.fn(),
    fetchPullRequest: vi.fn(),
    pullRequestFiles: vi.fn(),
    cloning: vi.fn(),
  },
  events: {
    changed: { listen: vi.fn(async () => () => {}) },
    headsFetched: { listen: vi.fn(async () => () => {}) },
    cloningChanged: { listen: vi.fn(async () => () => {}) },
  },
}));

const view: PrView = {
  number: 11,
  title: "gh-10: Fix the crash",
  author: "some-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: null,
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: null,
  content: null,
};

const done = { status: "ok" as const, data: null };

describe("PrViewState", () => {
  beforeEach(() => {
    vi.mocked(commands.openPullRequest).mockResolvedValue(done);
    vi.mocked(commands.fetchPullRequest).mockResolvedValue(done);
    vi.mocked(commands.pullRequest).mockResolvedValue({ status: "ok", data: view });
    vi.mocked(commands.pullRequestFiles).mockResolvedValue({ status: "ok", data: { state: "fetching" } });
    vi.mocked(commands.cloning).mockResolvedValue(null);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("opens the PR with the core, holds what it answers, and remembers the Issue it came from", async () => {
    const state = new PrViewState("python/cpython", 11, 10);

    await state.load();

    expect(commands.openPullRequest).toHaveBeenCalledWith("python/cpython", 11);
    expect(commands.pullRequest).toHaveBeenCalledWith("python/cpython", 11);
    expect(state.view?.title).toBe("gh-10: Fix the crash");
    expect(state.issue).toBe(10);
    expect(state.files.view).toEqual({ state: "fetching" });
    state.stop();
  });

  it("reads the PR from GitHub once a minute while it's on screen", async () => {
    vi.useFakeTimers();
    const state = new PrViewState("python/cpython", 11, undefined);
    await state.load();

    await vi.advanceTimersByTimeAsync(60_000);

    expect(commands.fetchPullRequest).toHaveBeenCalledExactlyOnceWith("python/cpython", 11);
    expect(commands.openPullRequest).toHaveBeenCalledTimes(1);
    state.stop();
  });

  it("opens on the Conversation and switches between its two tabs", () => {
    const state = new PrViewState("python/cpython", 11, undefined);
    expect(state.tab).toBe("conversation");

    state.switchTab();
    expect(state.tab).toBe("files");
    state.switchTab();
    expect(state.tab).toBe("conversation");
  });

  it("opens a thread on the Files tab", () => {
    const state = new PrViewState("python/cpython", 11, undefined);
    const reveal = vi.spyOn(state.files, "reveal");

    state.openThread("T1");

    expect(state.tab).toBe("files");
    expect(reveal).toHaveBeenCalledExactlyOnceWith("T1");
  });
});
