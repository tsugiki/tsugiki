import { render, screen } from "@testing-library/svelte";
import { SvelteDate } from "svelte/reactivity";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RepositoryState } from "$lib/state/repository-state.svelte";
import type { RepositorySync } from "$lib/state/repository-sync.svelte";
import { ok } from "$lib/testing/fixtures";
import { catchChanged } from "$lib/testing/state";
import RepositoryPage from "./RepositoryPage.svelte";

const core = vi.hoisted(() => ({ repositoryState: vi.fn(), repositorySync: vi.fn() }));
/** The listeners the page registers for the status and the last syncs. */
const listen = vi.hoisted(() => ({ status: vi.fn(), lastSync: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({
  commands: core,
  events: {
    changed: { listen: vi.fn() },
    statusChanged: { listen: listen.status },
    lastSyncChanged: { listen: listen.lastSync },
  },
}));

type LastSyncHeard = (event: {
  event: string;
  id: number;
  payload: { repository: string; at: string };
}) => void;

// SvelteDate makes its methods reactive from the Date there is when the first one is built, and a fake
// clock's Date has none of its own, so the page's clock would never move on. This builds that first one
// before any test fakes the clock.
new SvelteDate();

const writable: RepositoryState = { read_only: [], connection: "online" };
const readOnly: RepositoryState = { read_only: ["token"], connection: "online" };
const synced: RepositorySync = { last_sync: "2026-11-05T12:00:00Z", open: { issues: 1204, prs: 1 } };

/** Catches the listener for the last syncs, so a test can say one of `repository` finished, and its stop. */
const catchLastSync = () => {
  const stop = vi.fn();
  const send = vi.fn();
  listen.lastSync.mockImplementation(async (callback: LastSyncHeard) => {
    send.mockImplementation((repository: string, at: string) => {
      callback({ event: "last-sync-changed", id: 1, payload: { repository, at } });
    });
    return stop;
  });
  return { stop, send };
};

/** The parts of the header under the Repository's name, in order. */
const facts = () =>
  Array.from(document.querySelectorAll(".facts span")).map((part) => ({
    words: part.textContent,
    title: part.getAttribute("title"),
  }));
const words = () => facts().map((part) => part.words);

describe("a Repository's page in Settings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.useFakeTimers({ now: new Date("2026-11-05T12:02:00Z") });
    listen.status.mockResolvedValue(() => {});
    core.repositoryState.mockResolvedValue(ok(writable));
    core.repositorySync.mockResolvedValue(ok(synced));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("heads its page with its name, Can write, its open Issues and PRs and its last sync", async () => {
    catchChanged();
    catchLastSync();
    render(RepositoryPage, { repository: "python/cpython" });

    expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
    await vi.waitFor(() => {
      expect(words()).toEqual(["Can write", "1,204 open Issues · 1 open PR", "Synced 2 min ago"]);
    });
    expect(facts()[0]?.title).toBeNull();
    expect(core.repositoryState).toHaveBeenCalledWith("python/cpython");
    expect(core.repositorySync).toHaveBeenCalledWith("python/cpython");
  });

  it("says Read-only while the Repository is marked so, with why in its tooltip, and follows the mark", async () => {
    const changed = catchChanged();
    catchLastSync();
    core.repositoryState.mockResolvedValue(ok(readOnly));
    render(RepositoryPage, { repository: "python/cpython" });

    await vi.waitFor(() => {
      expect(facts()[0]).toEqual({
        words: "Read-only",
        title: "Read-only: your token can't write to python/cpython",
      });
    });
    core.repositoryState.mockResolvedValue(ok(writable));
    changed.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()[0]).toEqual({ words: "Can write", title: null });
    });
  });

  it("says when it hasn't synced yet, and no counts before a sync has counted them", async () => {
    catchChanged();
    catchLastSync();
    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
    render(RepositoryPage, { repository: "python/cpython" });

    await vi.waitFor(() => {
      expect(words()).toEqual(["Can write", "Not synced yet"]);
    });
  });

  it("moves its last sync on while the page shows, and asks again when a sync of its Repository finishes", async () => {
    catchChanged();
    const lastSync = catchLastSync();
    render(RepositoryPage, { repository: "python/cpython" });
    await vi.waitFor(() => {
      expect(words()).toContain("Synced 2 min ago");
    });

    await vi.advanceTimersByTimeAsync(60_000);
    expect(words()).toContain("Synced 3 min ago");

    const now = "2026-11-05T12:03:00Z";
    core.repositorySync.mockResolvedValue(ok({ last_sync: now, open: { issues: 1, prs: 0 } }));
    lastSync.send("python/peps", now);
    await vi.advanceTimersByTimeAsync(0);
    expect(core.repositorySync).toHaveBeenCalledTimes(1);
    lastSync.send("python/cpython", now);

    await vi.waitFor(() => {
      expect(words()).toEqual(["Can write", "1 open Issue · 0 open PRs", "Synced just now"]);
    });
  });

  it("asks about the Repository it shows after a switch, and stops listening and ticking when it goes", async () => {
    const changed = catchChanged();
    const lastSync = catchLastSync();
    const { rerender, unmount } = render(RepositoryPage, { repository: "python/cpython" });
    await vi.waitFor(() => {
      expect(words()).toHaveLength(3);
    });

    await rerender({ repository: "python/peps" });
    await vi.waitFor(() => {
      expect(core.repositorySync).toHaveBeenLastCalledWith("python/peps");
    });
    expect(screen.getByRole("heading", { name: "python/peps" })).toBeTruthy();
    expect(core.repositoryState).toHaveBeenLastCalledWith("python/peps");
    expect(changed.stop).toHaveBeenCalledTimes(1);
    expect(lastSync.stop).toHaveBeenCalledTimes(1);

    unmount();
    expect(changed.stop).toHaveBeenCalledTimes(2);
    expect(lastSync.stop).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);
  });
});
