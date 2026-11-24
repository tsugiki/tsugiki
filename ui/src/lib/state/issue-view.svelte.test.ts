import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { done, ok, refused } from "$lib/testing/fixtures";
import { catchChanged } from "$lib/testing/state";
import { IssueViewState, type IssueView } from "./issue-view.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { issue: vi.fn(), openIssue: vi.fn(), hideItem: vi.fn(), showItem: vi.fn(), closeItem: vi.fn() },
  events: {
    acted: { listen: vi.fn() },
    changed: { listen: vi.fn() },
    readFailed: { listen: vi.fn(async () => () => {}) },
  },
}));

const view = (title: string): IssueView => ({
  number: 7,
  title,
  state: "OPEN",
  people: ["issue-author"],
  mute_author: { kind: "mute", login: "issue-author" },
  pinned: false,
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [],
  merged: [],
  merged_count: 0,
  author: "issue-author",
  assignees: [],
  labels: [],
  needs_attention: [],
  hidden: null,
  content: null,
  seen: { mark: { numbers: [7], up_to: null, findings: [] }, state: "seen" },
});

describe("IssueViewState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(commands.openIssue).mockResolvedValue(done);
    for (const move of [commands.hideItem, commands.showItem, commands.closeItem])
      vi.mocked(move).mockResolvedValue(done);
    vi.mocked(commands.issue).mockResolvedValue(ok(view("First")));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("opens the Issue with the core and holds what it answers", async () => {
    catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    expect(state.view).toBeUndefined();

    await state.load();

    expect(commands.openIssue).toHaveBeenCalledWith("python/cpython", 7);
    expect(commands.issue).toHaveBeenCalledWith("python/cpython", 7);
    expect(state.view?.title).toBe("First");
    state.stop();
  });

  it("holds null for an Issue Tsugiki doesn't hold", async () => {
    catchChanged();
    vi.mocked(commands.issue).mockResolvedValue(ok(null));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.view).toBeNull();
    state.stop();
  });

  it("forgets why a read failed once a later read saves the Issue it doesn't hold", async () => {
    const { send } = catchChanged();
    vi.mocked(commands.issue).mockResolvedValue(ok(null));
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    state.failedRead = "waits_on_github";

    send("python/cpython", [8]);
    expect(state.failedRead).toBe("waits_on_github");
    send("python/cpython", [7]);

    expect(state.failedRead).toBeUndefined();
    state.stop();
  });

  it("keeps why a read failed when the Issue it holds changes, as Mark Seen changes it too", async () => {
    const { send } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    state.failedRead = "waits_on_github";

    send("python/cpython", [7]);

    expect(state.failedRead).toBe("waits_on_github");
    state.stop();
  });

  it("asks again when its Repository changes, and only then", async () => {
    const { send } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    vi.mocked(commands.issue).mockResolvedValue(ok(view("Second")));

    send("python/other");
    await vi.waitFor(() => {
      expect(commands.issue).toHaveBeenCalledTimes(1);
    });
    send("python/cpython");

    await vi.waitFor(() => {
      expect(state.view?.title).toBe("Second");
    });
    state.stop();
  });

  it("opening it again tells the core again", async () => {
    catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    await state.open();

    expect(commands.openIssue).toHaveBeenCalledTimes(2);
    state.stop();
  });

  it("leaves reading the Issue from GitHub again to the core, which hears when a PR hides it and it shows", async () => {
    vi.useFakeTimers();
    catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    await vi.advanceTimersByTimeAsync(120_000);
    state.hide();
    await vi.waitFor(() => {
      expect(commands.hideItem).toHaveBeenCalledExactlyOnceWith("python/cpython", 7);
    });
    state.show();
    await vi.waitFor(() => {
      expect(commands.showItem).toHaveBeenCalledExactlyOnceWith("python/cpython", 7);
    });

    expect(commands.openIssue).toHaveBeenCalledOnce();
    state.stop();
  });

  it("tells the core the user left the Issue once it stops", async () => {
    catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    state.stop();

    await vi.waitFor(() => {
      expect(commands.closeItem).toHaveBeenCalledExactlyOnceWith("python/cpython", 7);
    });
  });

  it("tells the core what the user did only once it has answered the opening", async () => {
    catchChanged();
    const opening = Promise.withResolvers<typeof done>();
    vi.mocked(commands.openIssue).mockReturnValueOnce(opening.promise);
    const state = new IssueViewState("python/cpython", 7);
    const loading = state.load();
    await vi.waitFor(() => {
      expect(commands.openIssue).toHaveBeenCalledOnce();
    });

    state.stop();
    await Promise.resolve();
    expect(commands.closeItem).not.toHaveBeenCalled();
    opening.resolve(done);
    await loading;

    await vi.waitFor(() => {
      expect(commands.closeItem).toHaveBeenCalledOnce();
    });
  });

  it("has nothing to catch up on once it shows, when it was opened again while hidden", async () => {
    const { send } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    state.hide();
    send("python/cpython");

    await state.open();
    state.show();

    expect(commands.issue).toHaveBeenCalledTimes(2);
    // Opening shows the Issue to the core, so the view says again that it's hidden, until it shows.
    await vi.waitFor(() => {
      expect(commands.showItem).toHaveBeenCalledOnce();
    });
    expect(commands.hideItem).toHaveBeenCalledTimes(2);
    // The core counts each view of an item once, so opening again closes first.
    expect(commands.closeItem).toHaveBeenCalledOnce();
    state.stop();
  });

  it("comes back as it was, without asking the core, when nothing changed while it was hidden", async () => {
    const { send } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    state.hide();
    send("python/other");
    state.show();
    await Promise.resolve();

    expect(state.view?.title).toBe("First");
    expect(commands.issue).toHaveBeenCalledOnce();
    state.stop();
  });

  it("asks the core again once it shows, when its Repository changed while it was hidden", async () => {
    const { send } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    vi.mocked(commands.issue).mockResolvedValue(ok(view("Second")));

    state.hide();
    send("python/cpython");
    send("python/cpython");
    await Promise.resolve();
    expect(commands.issue).toHaveBeenCalledOnce();
    state.show();

    await vi.waitFor(() => {
      expect(state.view?.title).toBe("Second");
    });
    expect(commands.issue).toHaveBeenCalledTimes(2);
    expect(commands.openIssue).toHaveBeenCalledOnce();
    state.stop();
  });

  it("stops listening when told to", async () => {
    const { stop } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    state.stop();

    expect(stop).toHaveBeenCalledOnce();
  });

  it("stops listening if it was told to while the listener was on its way", async () => {
    const { stop } = catchChanged();
    const state = new IssueViewState("python/cpython", 7);
    const loading = state.load();

    state.stop();
    await loading;

    expect(stop).toHaveBeenCalledOnce();
    expect(commands.openIssue).not.toHaveBeenCalled();
    expect(commands.closeItem).not.toHaveBeenCalled();
  });

  it("says so when it can't listen", async () => {
    vi.mocked(events.changed.listen).mockRejectedValue(new Error("no events"));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.error).toBe("Error: no events");
    expect(commands.openIssue).not.toHaveBeenCalled();
  });

  it("says why the core refused to open it, and the next good read clears only a read's error", async () => {
    catchChanged();
    vi.mocked(commands.openIssue).mockResolvedValue(refused("no database"));
    vi.mocked(commands.issue).mockResolvedValue(refused("no database"));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();
    expect(state.error).toBe("no database");

    vi.mocked(commands.openIssue).mockResolvedValue(done);
    vi.mocked(commands.issue).mockResolvedValue(ok(view("First")));
    await state.open();

    expect(state.error).toBeUndefined();
    state.stop();
  });

  it("says so when the core doesn't answer at all", async () => {
    catchChanged();
    vi.mocked(commands.openIssue).mockRejectedValue(new Error("no core"));
    vi.mocked(commands.issue).mockRejectedValue(new Error("no core"));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.error).toBe("Error: no core");
    state.stop();
  });

  it("drops an older answer that arrives late", async () => {
    const { send } = catchChanged();
    const older = Promise.withResolvers<ReturnType<typeof ok<IssueView>>>();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    vi.mocked(commands.issue).mockReturnValueOnce(older.promise);
    vi.mocked(commands.issue).mockResolvedValue(ok(view("Newer")));

    send("python/cpython");
    send("python/cpython");
    await vi.waitFor(() => {
      expect(state.view?.title).toBe("Newer");
    });
    older.resolve(ok(view("Older")));
    await Promise.resolve();

    expect(state.view?.title).toBe("Newer");
    state.stop();
  });
});
