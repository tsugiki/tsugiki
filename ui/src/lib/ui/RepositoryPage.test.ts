import { render, screen } from "@testing-library/svelte";
import { SvelteDate } from "svelte/reactivity";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RepositoryState } from "$lib/state/repository-state.svelte";
import type { RepositorySync } from "$lib/state/repository-sync.svelte";
import { ok, refused } from "$lib/testing/fixtures";
import RepositoryPage from "./RepositoryPage.svelte";

const core = vi.hoisted(() => ({ repositoryState: vi.fn(), repositorySync: vi.fn() }));
/** The listeners the page registers for the core's events. */
const listen = vi.hoisted(() => ({ changed: vi.fn(), status: vi.fn(), lastSync: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({
  commands: core,
  events: {
    changed: { listen: listen.changed },
    statusChanged: { listen: listen.status },
    lastSyncChanged: { listen: listen.lastSync },
  },
}));

type Heard = (event: { event: string; id: number; payload: { repository: string } }) => void;

// SvelteDate makes its methods reactive from the Date there is when the first one is built, and a fake
// clock's Date has none of its own, so the page's clock would never move on. This builds that first one
// before any test fakes the clock.
new SvelteDate();

const writable: RepositoryState = { read_only: [], connection: "online" };
const readOnly: RepositoryState = { read_only: ["token"], connection: "online" };
const synced: RepositorySync = { last_sync: "2026-11-05T12:00:00Z", open: { issues: 1204, prs: 1 } };

/** Catches every listener the page registers on `on`, so a test can send each of them an event naming
 * `repository`, as the core would, and counts the listeners stopped. Both of the page's views listen for a
 * Repository's changes. */
const catchAll = (on: typeof listen.changed, event: string) => {
  const heard: Heard[] = [];
  const stop = vi.fn();
  on.mockImplementation(async (callback: Heard) => {
    heard.push(callback);
    return stop;
  });
  const send = (repository: string) => {
    for (const callback of heard) callback({ event, id: 1, payload: { repository } });
  };
  return { send, stop };
};
const catchChanged = () => catchAll(listen.changed, "changed");
const catchLastSync = () => catchAll(listen.lastSync, "last-sync-changed");

/** The line under the Repository's name, or nothing while it shows none. */
const facts = () => document.querySelector(".facts")?.textContent ?? null;

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
      expect(facts()).toBe("Can write · 1,204 open Issues · 1 open PR · Synced 2 min ago");
    });
    expect(core.repositoryState).toHaveBeenCalledWith("python/cpython");
    expect(core.repositorySync).toHaveBeenCalledWith("python/cpython");
  });

  it("says Read-only and why while the Repository is marked so, and follows the mark", async () => {
    const changed = catchChanged();
    catchLastSync();
    core.repositoryState.mockResolvedValue(ok(readOnly));
    render(RepositoryPage, { repository: "python/cpython" });

    await vi.waitFor(() => {
      expect(facts()).toBe(
        "Read-only: your token can't write to python/cpython · 1,204 open Issues · 1 open PR · Synced 2 min ago",
      );
    });
    core.repositoryState.mockResolvedValue(ok(writable));
    changed.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toMatch(/^Can write · /);
    });
  });

  it("says when it hasn't synced yet, and no counts before a sync has counted them", async () => {
    catchChanged();
    catchLastSync();
    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
    render(RepositoryPage, { repository: "python/cpython" });

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · Not synced yet");
    });
  });

  it("shows no line while the core can't say any of it", async () => {
    catchChanged();
    catchLastSync();
    core.repositoryState.mockResolvedValue(refused("The database is locked."));
    core.repositorySync.mockResolvedValue(refused("The database is locked."));
    render(RepositoryPage, { repository: "python/cpython" });

    await vi.waitFor(() => {
      expect(core.repositorySync).toHaveBeenCalled();
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(facts()).toBeNull();
  });

  it("shows the counts a sync saves before it finishes, when the core says the Repository changed", async () => {
    const changed = catchChanged();
    catchLastSync();
    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
    render(RepositoryPage, { repository: "python/cpython" });
    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · Not synced yet");
    });

    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: { issues: 7, prs: 5 } }));
    changed.send("python/peps");
    await vi.advanceTimersByTimeAsync(0);
    expect(facts()).toBe("Can write · Not synced yet");
    changed.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 7 open Issues · 5 open PRs · Not synced yet");
    });
  });

  it("moves its last sync on while the page shows, and asks again when a sync of its Repository finishes", async () => {
    catchChanged();
    const lastSync = catchLastSync();
    render(RepositoryPage, { repository: "python/cpython" });
    await vi.waitFor(() => {
      expect(facts()).toContain("Synced 2 min ago");
    });

    await vi.advanceTimersByTimeAsync(60_000);
    expect(facts()).toContain("Synced 3 min ago");

    core.repositorySync.mockResolvedValue(
      ok({ last_sync: "2026-11-05T12:03:00Z", open: { issues: 1, prs: 0 } }),
    );
    lastSync.send("python/peps");
    await vi.advanceTimersByTimeAsync(0);
    expect(core.repositorySync).toHaveBeenCalledTimes(1);
    lastSync.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 1 open Issue · 0 open PRs · Synced just now");
    });
  });

  it("asks about the Repository it shows after a switch, and stops listening and ticking when it goes", async () => {
    const changed = catchChanged();
    const lastSync = catchLastSync();
    const { rerender, unmount } = render(RepositoryPage, { repository: "python/cpython" });
    await vi.waitFor(() => {
      expect(facts()).toContain("Synced");
    });

    await rerender({ repository: "python/peps" });
    await vi.waitFor(() => {
      expect(core.repositorySync).toHaveBeenLastCalledWith("python/peps");
    });
    expect(screen.getByRole("heading", { name: "python/peps" })).toBeTruthy();
    expect(core.repositoryState).toHaveBeenLastCalledWith("python/peps");
    // Both views listen for the Repository's changes, and only the sync's view for its last syncs.
    expect(changed.stop).toHaveBeenCalledTimes(2);
    expect(lastSync.stop).toHaveBeenCalledTimes(1);

    unmount();
    expect(changed.stop).toHaveBeenCalledTimes(4);
    expect(lastSync.stop).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);
  });
});
