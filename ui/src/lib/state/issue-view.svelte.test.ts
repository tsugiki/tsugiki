import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { IssueViewState, type IssueView } from "./issue-view.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { issue: vi.fn(), openIssue: vi.fn(), fetchIssue: vi.fn() },
  events: { changed: { listen: vi.fn() }, readFailed: { listen: vi.fn(async () => () => {}) } },
}));

const view = (title: string): IssueView => ({
  number: 7,
  title,
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [],
  author: "issue-author",
  content: null,
});

const ok = (data: IssueView | null) => ({ status: "ok" as const, data });
const done = { status: "ok" as const, data: null };
const refused = { status: "error" as const, error: { kind: "internal" as const, message: "no database" } };

/** Catches the listener the state registers, so a test can say a Repository changed as the core would. */
const catchListener = () => {
  const stop = vi.fn();
  const send = vi.fn();
  vi.mocked(events.changed.listen).mockImplementation(async (callback) => {
    send.mockImplementation((repository: string) => {
      callback({ event: "changed", id: 1, payload: { repository, items: [] } });
    });
    return stop;
  });
  return { stop, send };
};

describe("IssueViewState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(commands.openIssue).mockResolvedValue(done);
    vi.mocked(commands.fetchIssue).mockResolvedValue(done);
    vi.mocked(commands.issue).mockResolvedValue(ok(view("First")));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("opens the Issue with the core and holds what it answers", async () => {
    catchListener();
    const state = new IssueViewState("python/cpython", 7);
    expect(state.view).toBeUndefined();

    await state.load();

    expect(commands.openIssue).toHaveBeenCalledWith("python/cpython", 7);
    expect(commands.issue).toHaveBeenCalledWith("python/cpython", 7);
    expect(state.view?.title).toBe("First");
    state.stop();
  });

  it("holds null for an Issue Tsugiki doesn't hold", async () => {
    catchListener();
    vi.mocked(commands.issue).mockResolvedValue(ok(null));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.view).toBeNull();
    state.stop();
  });

  it("asks again when its Repository changes, and only then", async () => {
    const { send } = catchListener();
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

  it("opening it again moves Last seen again", async () => {
    catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    await state.open();

    expect(commands.openIssue).toHaveBeenCalledTimes(2);
    state.stop();
  });

  it("reads the Issue from GitHub once a minute while it's on screen, without moving Last seen", async () => {
    vi.useFakeTimers();
    catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    await vi.advanceTimersByTimeAsync(60_000);

    expect(commands.fetchIssue).toHaveBeenCalledExactlyOnceWith("python/cpython", 7);
    expect(commands.openIssue).toHaveBeenCalledTimes(1);
    state.stop();
  });

  it("leaves GitHub alone while the window is hidden, and once it stops", async () => {
    vi.useFakeTimers();
    catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);

    await vi.advanceTimersByTimeAsync(60_000);
    hidden.mockRestore();
    state.stop();
    await vi.advanceTimersByTimeAsync(60_000);

    expect(commands.fetchIssue).not.toHaveBeenCalled();
  });

  it("leaves GitHub alone while a PR hides it, and reads the minute it missed once it shows", async () => {
    vi.useFakeTimers();
    catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    state.hide();
    state.show();
    expect(commands.fetchIssue).not.toHaveBeenCalled();
    state.hide();
    await vi.advanceTimersByTimeAsync(120_000);
    expect(commands.fetchIssue).not.toHaveBeenCalled();
    state.show();
    expect(commands.fetchIssue).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(60_000);

    expect(commands.fetchIssue).toHaveBeenCalledTimes(2);
    expect(commands.openIssue).toHaveBeenCalledOnce();
    state.stop();
  });

  it("has nothing to catch up on once it shows, when it was opened again while hidden", async () => {
    vi.useFakeTimers();
    const { send } = catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();
    state.hide();
    send("python/cpython");
    await vi.advanceTimersByTimeAsync(60_000);

    await state.open();
    state.show();

    expect(commands.issue).toHaveBeenCalledTimes(2);
    expect(commands.fetchIssue).not.toHaveBeenCalled();
    state.stop();
  });

  it("comes back as it was, without asking the core, when nothing changed while it was hidden", async () => {
    const { send } = catchListener();
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
    const { send } = catchListener();
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
    const { stop } = catchListener();
    const state = new IssueViewState("python/cpython", 7);
    await state.load();

    state.stop();

    expect(stop).toHaveBeenCalledOnce();
  });

  it("stops listening if it was told to while the listener was on its way", async () => {
    const { stop } = catchListener();
    const state = new IssueViewState("python/cpython", 7);
    const loading = state.load();

    state.stop();
    await loading;

    expect(stop).toHaveBeenCalledOnce();
    expect(commands.openIssue).not.toHaveBeenCalled();
  });

  it("says so when it can't listen", async () => {
    vi.mocked(events.changed.listen).mockRejectedValue(new Error("no events"));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.error).toBe("Error: no events");
    expect(commands.openIssue).not.toHaveBeenCalled();
  });

  it("says why the core refused to open it, and the next good read clears only a read's error", async () => {
    catchListener();
    vi.mocked(commands.openIssue).mockResolvedValue(refused);
    vi.mocked(commands.issue).mockResolvedValue(refused);
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
    catchListener();
    vi.mocked(commands.openIssue).mockRejectedValue(new Error("no core"));
    vi.mocked(commands.issue).mockRejectedValue(new Error("no core"));
    const state = new IssueViewState("python/cpython", 7);

    await state.load();

    expect(state.error).toBe("Error: no core");
    state.stop();
  });

  it("drops an older answer that arrives late", async () => {
    const { send } = catchListener();
    const older = Promise.withResolvers<ReturnType<typeof ok>>();
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
