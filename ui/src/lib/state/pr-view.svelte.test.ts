import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events, type FailedRead } from "$lib/data/bindings";
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
    readFailed: { listen: vi.fn(async () => () => {}) },
    headsFetched: { listen: vi.fn(async () => () => {}) },
    headsFetchFailed: { listen: vi.fn(async () => () => {}) },
    cloningChanged: { listen: vi.fn(async () => () => {}) },
    mirrorStatesChanged: { listen: vi.fn(async () => () => {}) },
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
    state.stop();
  });

  it("shows the Conversation while opening the PR still waits for git", async () => {
    vi.mocked(commands.openPullRequest).mockReturnValue(new Promise(() => {}));
    const state = new PrViewState("python/cpython", 11, undefined);

    void state.load();

    await vi.waitFor(() => expect(state.view?.title).toBe("gh-10: Fix the crash"));
    state.stop();
  });

  it("leaves the diff unread while the PR is on its Conversation", async () => {
    const state = new PrViewState("python/cpython", 11, undefined);

    await state.load();

    expect(commands.pullRequestFiles).not.toHaveBeenCalled();
    expect(state.files.view).toBeUndefined();
    state.stop();
  });

  it("reads the diff the first time the Files tab shows, and keeps it after", async () => {
    const state = new PrViewState("python/cpython", 11, undefined);
    await state.load();

    state.switchTab();
    await vi.waitFor(() => expect(state.files.view).toEqual({ state: "fetching" }));
    state.switchTab();
    state.switchTab();

    expect(commands.pullRequestFiles).toHaveBeenCalledExactlyOnceWith("python/cpython", 11);
    state.stop();
  });

  it("reads the diff of a PR opened on a thread", async () => {
    const state = new PrViewState("python/cpython", 11, undefined);

    state.openThread("T1");
    await state.load();

    await vi.waitFor(() => expect(state.files.view).toEqual({ state: "fetching" }));
    expect(commands.pullRequestFiles).toHaveBeenCalledOnce();
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

  it("hears why a read of this PR from GitHub failed, and of no other item", async () => {
    let hear: ((failed: FailedRead) => void) | undefined;
    vi.mocked(events.readFailed.listen).mockImplementationOnce(async (callback) => {
      hear = (payload) => {
        callback({ event: "read-failed", id: 1, payload });
      };
      return () => {};
    });
    const state = new PrViewState("python/cpython", 11, undefined);
    await state.load();
    expect(state.failedRead).toBeUndefined();

    hear?.({ repository: "python/cpython", number: 12, why: "not_on_github" });
    hear?.({ repository: "python/other", number: 11, why: "not_on_github" });
    expect(state.failedRead).toBeUndefined();

    hear?.({ repository: "python/cpython", number: 11, why: "waits_on_github" });
    expect(state.failedRead).toBe("waits_on_github");
    state.stop();
  });

  it("stops the listener that started when the other can't, and says why", async () => {
    const stop = vi.fn();
    vi.mocked(events.changed.listen).mockResolvedValueOnce(stop);
    vi.mocked(events.readFailed.listen).mockRejectedValueOnce(new Error("no events"));
    const state = new PrViewState("python/cpython", 11, undefined);

    await state.load();

    expect(stop).toHaveBeenCalledOnce();
    expect(state.error).toBe("Error: no events");
    expect(commands.openPullRequest).not.toHaveBeenCalled();
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
