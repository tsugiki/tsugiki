import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RepositoryState } from "$lib/state/repository-state.svelte";
import type { RepositorySync } from "$lib/state/repository-sync.svelte";
import { SettingsDialog } from "$lib/state/settings-dialog.svelte";
import { SettingsState } from "$lib/state/settings.svelte";
import { ThemeState } from "$lib/state/theme.svelte";
import { ok, refused } from "$lib/testing/fixtures";
import { LIGHT_SIDE } from "$lib/testing/state";
import RepositoryPage from "./RepositoryPage.svelte";
import { stampOf } from "./time";

const core = vi.hoisted(() => ({ repositoryState: vi.fn(), repositorySync: vi.fn() }));
/** The listeners the page registers for the core's events. */
const listen = vi.hoisted(() => ({ changed: vi.fn(), status: vi.fn(), syncs: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({
  commands: core,
  events: {
    changed: { listen: listen.changed },
    statusChanged: { listen: listen.status },
    repositorySyncChanged: { listen: listen.syncs },
  },
}));

type Heard = (event: { event: string; id: number; payload: { repository: string } }) => void;

const writable: RepositoryState = { read_only: [], connection: "online" };
const readOnly: RepositoryState = { read_only: ["token"], connection: "online" };
const synced: RepositorySync = { last_sync: "2026-11-05T12:00:00Z", open: { issues: 1204, prs: 1 } };

/** Catches every listener the page registers on `on`, so a test can send each of them an event naming
 * `repository`, as the core would, and counts the listeners stopped. */
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
const catchSyncs = () => catchAll(listen.syncs, "repository-sync-changed");

/** The line under the Repository's name, or nothing while it shows none. */
const facts = () => document.querySelector(".facts")?.textContent ?? null;

/** A Settings dialog that hasn't read the files. */
const unread = () => new SettingsDialog(new SettingsState(), new ThemeState(LIGHT_SIDE));
/** The page of `repository` in `dialog`. */
const show = (repository: string, dialog = unread()) => render(RepositoryPage, { dialog, repository });

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
    catchSyncs();
    show("python/cpython");

    expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 1,204 open Issues · 1 open PR · Synced 2 min ago");
    });
    expect(core.repositoryState).toHaveBeenCalledWith("python/cpython");
    expect(core.repositorySync).toHaveBeenCalledWith("python/cpython");
  });

  it("says Read-only and why while the Repository is marked so, and follows the mark", async () => {
    const changed = catchChanged();
    catchSyncs();
    core.repositoryState.mockResolvedValue(ok(readOnly));
    show("python/cpython");

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

  it("gives its last sync the date and time in a tooltip", async () => {
    catchChanged();
    catchSyncs();
    show("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 1,204 open Issues · 1 open PR · Synced 2 min ago");
    });

    const line = document.querySelector(".facts span[title]");
    expect(line?.textContent).toBe("Synced 2 min ago");
    expect(line?.getAttribute("title")).toBe(`Last synced ${stampOf("2026-11-05T12:00:00Z")}`);
  });

  it("says when it hasn't synced yet, and no counts before a sync has counted them", async () => {
    catchChanged();
    catchSyncs();
    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
    show("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · Not synced yet");
    });
  });

  it("says when it last synced though the core can't say the rest", async () => {
    catchChanged();
    catchSyncs();
    core.repositoryState.mockResolvedValue(refused("The database is locked."));
    core.repositorySync.mockResolvedValue(ok({ last_sync: "2026-11-05T12:00:00Z", open: null }));
    show("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Synced 2 min ago");
    });
    const line = document.querySelector(".facts span[title]");
    expect(line?.getAttribute("title")).toBe(`Last synced ${stampOf("2026-11-05T12:00:00Z")}`);
  });

  it("says what the core has said before the sync's answer comes", async () => {
    catchChanged();
    catchSyncs();
    // The page reads the syncs beside the state, and says the state's part as soon as it has it.
    core.repositorySync.mockReturnValue(new Promise(() => {}));
    show("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write");
    });
  });

  it("shows no line while the core can't say any of it", async () => {
    catchChanged();
    catchSyncs();
    core.repositoryState.mockResolvedValue(refused("The database is locked."));
    core.repositorySync.mockResolvedValue(refused("The database is locked."));
    show("python/cpython");

    await vi.waitFor(() => {
      expect(core.repositorySync).toHaveBeenCalled();
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(facts()).toBeNull();
  });

  it("shows the counts a sync saves before it finishes, when the core says they changed", async () => {
    const changed = catchChanged();
    const syncs = catchSyncs();
    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
    show("python/cpython");
    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · Not synced yet");
    });

    core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: { issues: 7, prs: 5 } }));
    // A change to the Repository's Issues and PRs says nothing of its syncs.
    changed.send("python/cpython");
    syncs.send("python/peps");
    await vi.advanceTimersByTimeAsync(0);
    expect(facts()).toBe("Can write · Not synced yet");
    syncs.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 7 open Issues · 5 open PRs · Not synced yet");
    });
    // Nor does a new count say anything of what the user may do there.
    expect(core.repositoryState).toHaveBeenCalledTimes(2);
  });

  it("moves its last sync on while the page shows, and asks again when a sync of its Repository finishes", async () => {
    catchChanged();
    const syncs = catchSyncs();
    show("python/cpython");
    await vi.waitFor(() => {
      expect(facts()).toContain("Synced 2 min ago");
    });

    await vi.advanceTimersByTimeAsync(60_000);
    expect(facts()).toContain("Synced 3 min ago");

    core.repositorySync.mockResolvedValue(
      ok({ last_sync: "2026-11-05T12:03:00Z", open: { issues: 1, prs: 0 } }),
    );
    syncs.send("python/peps");
    await vi.advanceTimersByTimeAsync(0);
    expect(core.repositorySync).toHaveBeenCalledTimes(1);
    syncs.send("python/cpython");

    await vi.waitFor(() => {
      expect(facts()).toBe("Can write · 1 open Issue · 0 open PRs · Synced just now");
    });
  });

  it("asks about the Repository it shows after a switch, and stops listening and ticking when it goes", async () => {
    const changed = catchChanged();
    const syncs = catchSyncs();
    const { rerender, unmount } = show("python/cpython");
    await vi.waitFor(() => {
      expect(facts()).toContain("Synced");
    });

    await rerender({ repository: "python/peps" });
    await vi.waitFor(() => {
      expect(core.repositorySync).toHaveBeenLastCalledWith("python/peps");
    });
    expect(screen.getByRole("heading", { name: "python/peps" })).toBeTruthy();
    expect(core.repositoryState).toHaveBeenLastCalledWith("python/peps");
    expect(changed.stop).toHaveBeenCalledTimes(1);
    expect(syncs.stop).toHaveBeenCalledTimes(1);

    unmount();
    expect(changed.stop).toHaveBeenCalledTimes(2);
    expect(syncs.stop).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("a Repository's Filters in Settings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    for (const on of Object.values(listen)) on.mockResolvedValue(() => {});
    core.repositoryState.mockResolvedValue(ok(writable));
    core.repositorySync.mockResolvedValue(ok(synced));
  });

  /** A dialog that has read python/cpython's Bugs and Docs Filters. */
  const read = () => {
    const dialog = unread();
    dialog.filters.take([
      {
        name: "python/cpython",
        filters: [
          { name: "Bugs", query: "label:bug", on_workbench: true },
          { name: "Docs", query: "label:docs", on_workbench: false },
        ],
      },
    ]);
    return dialog;
  };
  const filter = (at: number) => screen.getByRole("group", { name: `Filter ${at}` });
  const field = (at: number, name: "Name" | "Search") =>
    within(filter(at)).getByRole<HTMLInputElement>("textbox", { name });
  const press = (at: number, name: string) => {
    fireEvent.click(within(filter(at)).getByRole("button", { name }));
  };
  /** Each Filter's name and search, as the page shows them. */
  const shown = () =>
    screen.queryAllByRole("group").map((group) => {
      const [name, search] = within(group).getAllByRole<HTMLInputElement>("textbox");
      return [name?.value, search?.value];
    });

  it("shows no Filters before the dialog has read the files", () => {
    show("python/cpython");
    expect(screen.getByRole("heading", { name: /^Filters/ })).toBeTruthy();
    expect(shown()).toEqual([]);
    expect(screen.queryByRole("button", { name: "Add filter" })).toBeNull();
  });

  it("lists each Filter's name and search, and moves one up or down but not past either end", async () => {
    show("python/cpython", read());
    expect(shown()).toEqual([
      ["Bugs", "label:bug"],
      ["Docs", "label:docs"],
    ]);
    expect(within(filter(1)).getByRole("button", { name: "Move up" }).hasAttribute("disabled")).toBe(true);
    expect(within(filter(2)).getByRole("button", { name: "Move down" }).hasAttribute("disabled")).toBe(true);

    press(1, "Move down");
    await vi.waitFor(() => {
      expect(shown()).toEqual([
        ["Docs", "label:docs"],
        ["Bugs", "label:bug"],
      ]);
    });
    press(2, "Move up");
    await vi.waitFor(() => {
      expect(shown()[0]).toEqual(["Bugs", "label:bug"]);
    });
  });

  it("edits a name and a search in place, deletes a Filter, and turns a row with a blank one red", async () => {
    const dialog = read();
    show("python/cpython", dialog);

    await fireEvent.input(field(1, "Name"), { target: { value: "Bug reports" } });
    await fireEvent.input(field(2, "Search"), { target: { value: "" } });
    expect(filter(2).classList.contains("blank")).toBe(true);
    expect(field(2, "Search").getAttribute("aria-invalid")).toBe("true");
    expect(field(2, "Name").getAttribute("aria-invalid")).toBe("false");
    expect(dialog.unsaveable).toBeDefined();
    press(2, "Delete");

    await vi.waitFor(() => {
      expect(shown()).toEqual([["Bug reports", "label:bug"]]);
    });
    expect(filter(1).classList.contains("blank")).toBe(false);
    expect(dialog.filters.changes).toEqual([
      {
        repository: "python/cpython",
        read: [
          { name: "Bugs", query: "label:bug", on_workbench: true },
          { name: "Docs", query: "label:docs", on_workbench: false },
        ],
        filters: [{ name: "Bug reports", query: "label:bug", was: "label:bug" }],
      },
    ]);
  });

  it("adds a Filter with a blank name and search, red, with the focus in its name", async () => {
    const dialog = read();
    show("python/cpython", dialog);

    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));

    await vi.waitFor(() => {
      expect(document.activeElement).toBe(field(3, "Name"));
    });
    expect(filter(3).classList.contains("blank")).toBe(true);
    await fireEvent.input(field(3, "Name"), { target: { value: "Mine" } });
    await fireEvent.input(field(3, "Search"), { target: { value: "author:@me" } });
    expect(filter(3).classList.contains("blank")).toBe(false);
    expect(dialog.unsaveable).toBeUndefined();
  });

  it("turns the Filters off while Apply saves", async () => {
    const dialog = read();
    show("python/cpython", dialog);

    dialog.applying = true;

    await vi.waitFor(() => {
      expect(field(1, "Name").hasAttribute("disabled")).toBe(true);
    });
    for (const button of within(filter(1)).getAllByRole("button")) {
      expect(button.hasAttribute("disabled")).toBe(true);
    }
    expect(field(2, "Search").hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Add filter" }).hasAttribute("disabled")).toBe(true);
  });
});
