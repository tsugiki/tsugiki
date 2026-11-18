import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events, type FailedRead } from "$lib/data/bindings";
import { diffFile, done, ok, readyFiles, workbenchRow } from "$lib/testing/fixtures";
import { PanesState } from "./panes.svelte";
import { PrViewState, type PrView } from "./pr-view.svelte";

vi.mock("$lib/data/bindings", async () => {
  const { prFilesEvents } = await import("$lib/testing/fixtures");
  return {
    commands: {
      pullRequest: vi.fn(),
      openPullRequest: vi.fn(),
      fetchPullRequest: vi.fn(),
      pullRequestFiles: vi.fn(),
      cloning: vi.fn(),
      mirrorStates: vi.fn(async () => []),
      mirrorsHeld: vi.fn(async () => false),
      textSearch: vi.fn(async () => ({ status: "ok", data: { kind: "running", id: 1 } })),
      textSearchState: vi.fn(async () => ({ state: "no_checkout", id: 1 })),
      textSearchCancel: vi.fn(async () => null),
    },
    events: {
      ...prFilesEvents(() => vi.fn(async () => () => {})),
      textSearchChanged: { listen: vi.fn(async () => () => {}) },
    },
  };
});

const view: PrView = {
  ...workbenchRow(11, { title: "gh-10: Fix the crash", author: "some-author", ci: null }),
  content: null,
  shares_review_of: null,
  hidden: null,
};

describe("PrViewState", () => {
  beforeEach(() => {
    vi.mocked(commands.openPullRequest).mockResolvedValue(done);
    vi.mocked(commands.fetchPullRequest).mockResolvedValue(done);
    vi.mocked(commands.pullRequest).mockResolvedValue(ok(view));
    vi.mocked(commands.pullRequestFiles).mockResolvedValue({ status: "ok", data: { state: "fetching" } });
    vi.mocked(commands.cloning).mockResolvedValue(null);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("opens the PR with the core, holds what it answers, and remembers the Issue it came from", async () => {
    const state = new PrViewState("python/cpython", 11, 10, new PanesState());

    await state.load();

    expect(commands.openPullRequest).toHaveBeenCalledWith("python/cpython", 11);
    expect(commands.pullRequest).toHaveBeenCalledWith("python/cpython", 11);
    expect(state.view?.title).toBe("gh-10: Fix the crash");
    expect(state.issue).toBe(10);
    state.stop();
  });

  it("shows the Conversation while opening the PR still waits for git", async () => {
    vi.mocked(commands.openPullRequest).mockReturnValue(new Promise(() => {}));
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());

    void state.load();

    await vi.waitFor(() => expect(state.view?.title).toBe("gh-10: Fix the crash"));
    state.stop();
  });

  it("leaves the diff unread while the PR is on its Conversation", async () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());

    await state.load();

    expect(commands.pullRequestFiles).not.toHaveBeenCalled();
    expect(state.files.view).toBeUndefined();
    state.stop();
  });

  it("reads the diff the first time the Files tab shows, and keeps it after", async () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    await state.load();

    state.switchTab();
    await vi.waitFor(() => expect(state.files.view).toEqual({ state: "fetching" }));
    state.switchTab();
    state.switchTab();

    expect(commands.pullRequestFiles).toHaveBeenCalledExactlyOnceWith("python/cpython", 11);
    state.stop();
  });

  it("reads the diff of a PR opened on a thread", async () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());

    state.openThread("T1");
    await state.load();

    await vi.waitFor(() => expect(state.files.view).toEqual({ state: "fetching" }));
    expect(commands.pullRequestFiles).toHaveBeenCalledOnce();
    state.stop();
  });

  it("reads the PR from GitHub once a minute while it's on screen", async () => {
    vi.useFakeTimers();
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
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
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
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
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());

    await state.load();

    expect(stop).toHaveBeenCalledOnce();
    expect(state.error).toBe("Error: no events");
    expect(commands.openPullRequest).not.toHaveBeenCalled();
  });

  it("opens on the Conversation and switches between its two tabs", () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    expect(state.tab).toBe("conversation");

    state.switchTab();
    expect(state.tab).toBe("files");
    state.switchTab();
    expect(state.tab).toBe("conversation");
  });

  it("opens its Files tab on all the files, or one at a time when the settings say so", () => {
    expect(new PrViewState("python/cpython", 11, undefined, new PanesState()).files.mode).toBe("all");
    expect(
      new PrViewState("python/cpython", 11, undefined, new PanesState(), undefined, "one").files.mode,
    ).toBe("one");
  });

  it("opens a thread on the Files tab", () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    const reveal = vi.spyOn(state.files, "reveal");

    state.openThread("T1");

    expect(state.tab).toBe("files");
    expect(reveal).toHaveBeenCalledExactlyOnceWith("T1");
  });

  it("opens the Files tab at the files changed since the Stale review the PR view read", async () => {
    const stale: PrView = {
      ...view,
      your_review: {
        kind: "stale_review",
        reviewed: "1".repeat(40),
        head: "abc",
        files: [{ path: "Lib/os.py", whitespace_only: false }],
      },
    };
    vi.mocked(commands.pullRequest).mockResolvedValue(ok(stale));
    vi.mocked(commands.pullRequestFiles).mockResolvedValue(
      ok(readyFiles(6, { files: [diffFile("Lib/ast.py", 0), diffFile("Lib/os.py", 3)] })),
    );
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    await state.load();

    state.reviewChanges();

    expect(state.tab).toBe("files");
    await vi.waitFor(() => expect(state.files.jump).toEqual({ page: { kind: "file", file: 1 } }));
    expect([state.files.sinceReview(0), state.files.sinceReview(1)]).toEqual(["unchanged", "changed"]);
    state.stop();
  });

  it("reads the diff once for the rail's Search on the Conversation, and opens a match on the Files tab", async () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    const opened = vi.spyOn(state.files, "openMatch").mockResolvedValue();

    state.loadFiles();
    state.loadFiles();
    const onConversation = state.tab;
    await state.openMatch({ path: "Lib/os.py", line: 12, ends: { base: "b0", head: "abc" } });

    expect(onConversation).toBe("conversation");
    await vi.waitFor(() => expect(commands.pullRequestFiles).toHaveBeenCalled());
    expect(commands.pullRequestFiles).toHaveBeenCalledOnce();
    expect(state.tab).toBe("files");
    expect(opened).toHaveBeenCalledWith("Lib/os.py", 12, { base: "b0", head: "abc" });
    state.stop();
  });

  it("opens a file quick open found on the Files tab", () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    const opened = vi.spyOn(state.files, "openPath").mockReturnValue();

    state.openFile("Lib/os.py");

    expect(state.tab).toBe("files");
    expect(opened).toHaveBeenCalledWith("Lib/os.py");
    state.stop();
  });

  it("asks the Files tab for the diff again when the rail's Search finds no checkout of it", async () => {
    const state = new PrViewState("python/cpython", 11, undefined, new PanesState());
    const asked = vi.spyOn(state.files, "ask").mockResolvedValue();

    state.search.follow({ kind: "ready", base: "b0", head: "abc" });
    state.search.seed("spam");

    await vi.waitFor(() => expect(asked).toHaveBeenCalledOnce());
    state.stop();
  });
});
