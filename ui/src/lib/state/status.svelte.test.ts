import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, events } from "$lib/data/bindings";
import { StatusState, type Status } from "./status.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { status: vi.fn() },
  events: { statusChanged: { listen: vi.fn() } },
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
