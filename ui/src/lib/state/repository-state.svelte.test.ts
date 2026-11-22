import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { ok, refused } from "$lib/testing/fixtures";
import { catchChanged } from "$lib/testing/state";
import { readOnlyWords, type RepositoryState, RepositoryStateView } from "./repository-state.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { repositoryState: vi.fn() },
  events: { changed: { listen: vi.fn() }, statusChanged: { listen: vi.fn() } },
}));

const writable: RepositoryState = { read_only: [], connection: "online" };
const readOnly: RepositoryState = { read_only: ["token"], connection: "online" };
const offline: RepositoryState = { read_only: [], connection: "offline" };

/** Catches the status's listener, and gives what sends it a status and what stops it. */
const catchStatus = () => {
  const stop = vi.fn();
  const send = vi.fn();
  vi.mocked(events.statusChanged.listen).mockImplementation(async (callback) => {
    send.mockImplementation(() => {
      callback({ event: "statusChanged", id: 1, payload: { state: "offline" } });
    });
    return stop;
  });
  return { stop, send };
};

describe("RepositoryStateView", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(commands.repositoryState).mockResolvedValue(ok(readOnly));
  });

  it("holds what the core says of its Repository", async () => {
    catchChanged();
    catchStatus();
    const view = new RepositoryStateView("python/cpython");
    expect(view.state).toBeUndefined();

    await view.load();

    expect(commands.repositoryState).toHaveBeenCalledWith("python/cpython");
    expect(view.state).toEqual(readOnly);
  });

  it("asks again when the core says its Repository changed, and not for another", async () => {
    const changed = catchChanged();
    catchStatus();
    const view = new RepositoryStateView("python/cpython");
    await view.load();
    vi.mocked(commands.repositoryState).mockResolvedValue(ok(writable));

    changed.send("python/peps");
    await vi.waitFor(() => {
      expect(commands.repositoryState).toHaveBeenCalledTimes(1);
    });
    changed.send("python/cpython");

    await vi.waitFor(() => {
      expect(view.state).toEqual(writable);
    });
  });

  it("asks again when the status changes, which can say Tsugiki went offline", async () => {
    catchChanged();
    const status = catchStatus();
    const view = new RepositoryStateView("python/cpython");
    await view.load();
    vi.mocked(commands.repositoryState).mockResolvedValue(ok(offline));

    status.send();

    await vi.waitFor(() => {
      expect(view.state).toEqual(offline);
    });
  });

  it("drops an older answer that comes after a newer one", async () => {
    const changed = catchChanged();
    catchStatus();
    let answerFirst: (state: RepositoryState) => void = () => {};
    vi.mocked(commands.repositoryState)
      .mockReturnValueOnce(new Promise((resolve) => (answerFirst = (state) => resolve(ok(state)))))
      .mockResolvedValue(ok(writable));
    const view = new RepositoryStateView("python/cpython");
    const loading = view.load();
    await vi.waitFor(() => {
      expect(commands.repositoryState).toHaveBeenCalledOnce();
    });

    changed.send("python/cpython");
    await vi.waitFor(() => {
      expect(view.state).toEqual(writable);
    });
    answerFirst(readOnly);
    await loading;

    expect(view.state).toEqual(writable);
  });

  it("holds nothing while the core can't say", async () => {
    catchChanged();
    catchStatus();
    vi.mocked(commands.repositoryState).mockResolvedValue(refused("no database"));
    const view = new RepositoryStateView("python/cpython");

    await view.load();

    expect(view.state).toBeUndefined();
  });

  it("asks nothing without both listeners, and stops the one that started", async () => {
    const changed = catchChanged();
    vi.mocked(events.statusChanged.listen).mockRejectedValue(new Error("no events"));
    const view = new RepositoryStateView("python/cpython");

    await view.load();

    expect(commands.repositoryState).not.toHaveBeenCalled();
    expect(changed.stop).toHaveBeenCalledOnce();
    expect(view.state).toBeUndefined();
  });

  it("stops listening when it stops, and drops what comes after", async () => {
    const changed = catchChanged();
    const status = catchStatus();
    const view = new RepositoryStateView("python/cpython");
    await view.load();
    vi.mocked(commands.repositoryState).mockResolvedValue(ok(writable));

    changed.send("python/cpython");
    view.stop();
    await Promise.resolve();

    expect(changed.stop).toHaveBeenCalledOnce();
    expect(status.stop).toHaveBeenCalledOnce();
    expect(view.state).toEqual(readOnly);
  });

  it("stops the listeners at once when it stopped while they started", async () => {
    const changed = catchChanged();
    const status = catchStatus();
    const view = new RepositoryStateView("python/cpython");

    const loading = view.load();
    view.stop();
    await loading;

    expect(changed.stop).toHaveBeenCalledOnce();
    expect(status.stop).toHaveBeenCalledOnce();
    expect(commands.repositoryState).not.toHaveBeenCalled();
  });
});

describe("readOnlyWords", () => {
  it("says why Tsugiki can't write to the Repository", () => {
    expect(readOnlyWords(readOnly, "python/cpython")).toBe(
      "Read-only: your token can't write to python/cpython",
    );
  });

  it("says the user chose Read-only, and gives each reason in the core's order", () => {
    const chosen: RepositoryState = { read_only: ["chosen"], connection: "online" };
    const both: RepositoryState = { read_only: ["chosen", "token"], connection: "online" };
    expect(readOnlyWords(chosen, "python/cpython")).toBe(
      "Read-only: you chose it in this Repository's settings",
    );
    expect(readOnlyWords(both, "python/cpython")).toBe(
      "Read-only: you chose it in this Repository's settings, and your token can't write to python/cpython",
    );
  });

  it("says nothing while Tsugiki can write there, offline too, or before the core says", () => {
    expect(readOnlyWords(writable, "python/cpython")).toBeNull();
    expect(readOnlyWords(offline, "python/cpython")).toBeNull();
    expect(readOnlyWords(undefined, "python/cpython")).toBeNull();
  });
});
