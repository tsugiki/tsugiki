import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { StatusState, type Status } from "./status.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { status: vi.fn(), lastSync: vi.fn(), syncActivity: vi.fn(), refresh: vi.fn() },
  events: {
    statusChanged: { listen: vi.fn() },
    lastSyncChanged: { listen: vi.fn() },
    syncActivityChanged: { listen: vi.fn() },
  },
}));

const online: Status = { state: "online", login: "octocat" };
const offline: Status = { state: "offline" };

/** Catches the listener StatusState registers, so a test can send it events as the core would. */
const catchListener = () => {
  const listener = vi.fn();
  vi.mocked(events.statusChanged.listen).mockImplementation(async (callback) => {
    listener.mockImplementation((payload: Status) => {
      callback({ event: "status-changed", id: 1, payload });
    });
    return () => {};
  });
  return listener;
};

const synced = { repository: "python/cpython", at: "2026-11-05T12:00:00Z" };

/** Catches the listener for the last sync, as `catchListener` does for the status. */
const catchLastSync = () => {
  const listener = vi.fn();
  vi.mocked(events.lastSyncChanged.listen).mockImplementation(async (callback) => {
    listener.mockImplementation((payload: typeof synced) => {
      callback({ event: "last-sync-changed", id: 1, payload });
    });
    return () => {};
  });
  return listener;
};

describe("StatusState's last sync", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    catchListener();
    vi.mocked(commands.status).mockResolvedValue(online);
  });

  describe("the sync under way", () => {
    const activity = {
      open: { repository: "python/cpython", part: "window", pages: 3, load: null },
      paused_until: null,
      elsewhere: [] as string[],
    } as const;
    const catchActivity = () => {
      const listener = vi.fn<(payload: typeof activity | null) => void>();
      vi.mocked(events.syncActivityChanged.listen).mockImplementation(async (callback) => {
        listener.mockImplementation((payload) => {
          callback({ event: "sync-activity-changed", id: 1, payload });
        });
        return () => {};
      });
      return listener;
    };

    it("holds what the core says when the window loads", async () => {
      catchLastSync();
      catchActivity();
      vi.mocked(commands.syncActivity).mockResolvedValue(activity);
      const state = new StatusState();
      await state.load();
      expect(state.syncActivity).toEqual(activity);
    });

    it("follows the core, down to nothing", async () => {
      catchLastSync();
      const activityChanged = catchActivity();
      vi.mocked(commands.syncActivity).mockResolvedValue(activity);
      const state = new StatusState();
      await state.load();
      activityChanged(null);
      expect(state.syncActivity).toBeNull();
    });

    it("lets an event that arrives while asking win, even one that says nothing", async () => {
      catchLastSync();
      const activityChanged = catchActivity();
      vi.mocked(commands.syncActivity).mockImplementation(async () => {
        activityChanged(null);
        return activity;
      });
      const state = new StatusState();
      await state.load();
      expect(state.syncActivity).toBeNull();
    });
  });

  it("holds what the core says when the window loads", async () => {
    catchLastSync();
    vi.mocked(commands.lastSync).mockResolvedValue(synced);
    const state = new StatusState();
    expect(state.lastSync).toBeNull();
    await state.load();
    expect(state.lastSync).toEqual(synced);
  });

  it("follows the core's events", async () => {
    const send = catchLastSync();
    vi.mocked(commands.lastSync).mockResolvedValue(null);
    const state = new StatusState();
    await state.load();
    expect(state.lastSync).toBeNull();
    send(synced);
    expect(state.lastSync).toEqual(synced);
  });

  it("keeps an event that arrives while it is still asking", async () => {
    const send = catchLastSync();
    vi.mocked(commands.lastSync).mockImplementation(async () => {
      send(synced);
      return { repository: "python/cpython", at: null };
    });
    const state = new StatusState();
    await state.load();
    expect(state.lastSync).toEqual(synced);
  });

  it("asks the core to refresh", async () => {
    vi.mocked(commands.refresh).mockResolvedValue(undefined);
    const state = new StatusState();
    await state.refresh();
    expect(commands.refresh).toHaveBeenCalledOnce();
    expect(state.error).toBeUndefined();
  });

  it("says when it can't ask", async () => {
    vi.mocked(commands.refresh).mockRejectedValue(new Error("no IPC"));
    const state = new StatusState();
    await state.refresh();
    expect(state.error).toBe("Error: no IPC");
  });
});

describe("StatusState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("holds what the core says when the window loads", async () => {
    catchListener();
    vi.mocked(commands.status).mockResolvedValue(online);
    const state = new StatusState();
    expect(state.status).toBeUndefined();
    await state.load();
    expect(state.status).toEqual(online);
    expect(state.error).toBeUndefined();
  });

  it("follows the core's events", async () => {
    const send = catchListener();
    vi.mocked(commands.status).mockResolvedValue(online);
    const state = new StatusState();
    await state.load();
    send(offline);
    expect(state.status).toEqual(offline);
  });

  it("keeps an event that arrives while it is still asking, since that is newer", async () => {
    const send = catchListener();
    vi.mocked(commands.status).mockImplementation(async () => {
      send(online);
      return offline;
    });
    const state = new StatusState();
    await state.load();
    expect(state.status).toEqual(online);
  });

  it("listens before it asks, so no change is lost in between", async () => {
    catchListener();
    vi.mocked(commands.status).mockResolvedValue(online);
    await new StatusState().load();
    const listened = vi.mocked(events.statusChanged.listen).mock.invocationCallOrder[0];
    const asked = vi.mocked(commands.status).mock.invocationCallOrder[0];
    expect(listened).toBeLessThan(asked as number);
  });

  it("forgets a failed question when the core's next event arrives", async () => {
    const send = catchListener();
    vi.mocked(commands.status).mockRejectedValue(new Error("not yet"));
    const state = new StatusState();
    await state.load();
    expect(state.error).toBe("Error: not yet");
    send(online);
    expect(state.status).toEqual(online);
    expect(state.error).toBeUndefined();
  });

  it("holds the error when the core can't be reached", async () => {
    vi.mocked(events.statusChanged.listen).mockRejectedValue(new Error("no IPC outside Tauri"));
    const state = new StatusState();
    await state.load();
    expect(state.status).toBeUndefined();
    expect(state.error).toBe("Error: no IPC outside Tauri");
  });
});
