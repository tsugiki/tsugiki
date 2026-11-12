import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { openLink } from "$lib/state/browser";
import StatusBar from "./StatusBar.svelte";

vi.mock("$lib/state/browser", () => ({ openLink: vi.fn() }));

const bar = () => screen.getByRole("status");

describe("StatusBar's last sync", () => {
  const online = { state: "online", login: "octocat" } as const;
  const repository = "python/cpython";

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows nothing before the core has a Repository open", () => {
    render(StatusBar, { status: online });
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("says when the Repository last synced, outside the live region", () => {
    vi.useFakeTimers({ now: new Date("2026-11-05T12:02:00Z") });
    const lastSync = { repository, at: "2026-11-05T12:00:00Z" };
    render(StatusBar, { status: online, lastSync });
    expect(screen.getByRole("button").textContent).toBe("Synced 2 min ago");
    expect(bar().textContent).not.toContain("Synced");
    expect(screen.getByRole("button").parentElement).toBe(bar().parentElement);
  });

  it("moves on by itself", async () => {
    vi.useFakeTimers({ now: new Date("2026-11-05T12:02:00Z") });
    const lastSync = { repository, at: "2026-11-05T12:00:00Z" };
    render(StatusBar, { status: online, lastSync });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(screen.getByRole("button").textContent).toBe("Synced 3 min ago");
  });

  it("stops its clock when it goes", () => {
    vi.useFakeTimers();
    const { unmount } = render(StatusBar, { status: online, lastSync: { repository, at: null } });
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("refreshes at a click", async () => {
    const onrefresh = vi.fn();
    render(StatusBar, { status: online, lastSync: { repository, at: null }, onrefresh });
    await fireEvent.click(screen.getByRole("button", { name: "Not synced yet" }));
    expect(onrefresh).toHaveBeenCalledOnce();
  });

  it("has nothing to do at a click unless the page says", async () => {
    render(StatusBar, { status: online, lastSync: { repository, at: null } });
    await fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("button")).toBeTruthy();
  });

  it("stays out of the way when the settings hide the bar", () => {
    render(StatusBar, { status: online, lastSync: { repository, at: null }, visible: false });
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("stays beside Offline, tinted like the bar", () => {
    render(StatusBar, { status: { state: "offline" }, lastSync: { repository, at: null } });
    expect(screen.getByRole("button").classList.contains("offline")).toBe(true);
  });
});

describe("StatusBar", () => {
  it("shows who is signed in", () => {
    render(StatusBar, { status: { state: "online", login: "octocat" } });
    expect(bar().textContent).toContain("octocat");
    expect(bar().classList.contains("offline")).toBe(false);
  });

  it("says offline and tints the bar", () => {
    render(StatusBar, { status: { state: "offline" } });
    expect(bar().textContent).toContain("Offline: read-only");
    expect(bar().classList.contains("offline")).toBe(true);
  });

  it("shows who it was last time while it connects", () => {
    render(StatusBar, { status: { state: "connecting_as", last_login: "octocat" } });
    expect(bar().textContent?.trim()).toBe("octocat · Connecting…");
    expect(bar().classList.contains("offline")).toBe(false);
  });

  it("shows who it was last time while offline, and tints the bar", () => {
    render(StatusBar, { status: { state: "offline_as", last_login: "octocat" } });
    expect(bar().textContent?.trim()).toBe("octocat · Offline: read-only");
    expect(bar().classList.contains("offline")).toBe(true);
  });

  it("shows what stops Tsugiki reading, in the core's words", () => {
    const problem = { kind: "unauthorized", message: "Run `gh auth login`." } as const;
    render(StatusBar, { status: { state: "problem", problem } });
    expect(bar().textContent).toContain("Run `gh auth login`.");
  });

  it("waits while the core is connecting", () => {
    render(StatusBar, { status: { state: "connecting" } });
    expect(bar().textContent).toContain("Connecting…");
  });

  it("waits until the core has answered", () => {
    render(StatusBar, { status: undefined });
    expect(bar().textContent).toContain("Connecting…");
  });

  it("says when it can't read the status", () => {
    render(StatusBar, { status: undefined, error: "Error: no IPC" });
    expect(bar().textContent).toContain("Tsugiki can't read its status: Error: no IPC");
  });

  describe("when the settings hide it", () => {
    const online = { state: "online", login: "octocat" } as const;

    // It stays in the page, empty: screen readers only announce changes in a live region that already exists.
    const expectEmpty = () => {
      expect(bar().textContent?.trim()).toBe("");
      expect(bar().classList.contains("empty")).toBe(true);
    };

    it("is empty while all is well", () => {
      render(StatusBar, { status: online, visible: false });
      expectEmpty();
    });

    it("stays empty while connecting", () => {
      render(StatusBar, { status: { state: "connecting" }, visible: false });
      expectEmpty();
    });

    it("isn't empty when it has something to say", () => {
      render(StatusBar, { status: { state: "offline" }, visible: false });
      expect(bar().classList.contains("empty")).toBe(false);
    });

    it("comes back to say Tsugiki is offline", () => {
      render(StatusBar, { status: { state: "offline" }, visible: false });
      expect(bar().textContent?.trim()).toBe("Offline: read-only");
    });

    it("comes back to say who it was last time while offline", () => {
      render(StatusBar, { status: { state: "offline_as", last_login: "octocat" }, visible: false });
      expect(bar().textContent?.trim()).toBe("octocat · Offline: read-only");
    });

    it("comes back for a problem", () => {
      const problem = { kind: "unauthorized", message: "Run `gh auth login`." } as const;
      render(StatusBar, { status: { state: "problem", problem }, visible: false });
      expect(bar().textContent?.trim()).toBe("Run `gh auth login`.");
    });

    it("comes back when it can't read the status", () => {
      render(StatusBar, { status: undefined, error: "Error: no IPC", visible: false });
      expect(bar().textContent).toContain("Tsugiki can't read its status");
    });

    it("comes back with only what is wrong with the settings files", () => {
      render(StatusBar, { status: online, visible: false, problems: ["settings.toml, line 3: bad"] });
      expect(bar().textContent?.trim()).toBe("settings.toml, line 3: bad");
    });
  });

  describe("what is wrong with the settings files", () => {
    it("shows beside the status", () => {
      const status = { state: "online", login: "octocat" } as const;
      render(StatusBar, { status, problems: ["settings.toml, line 3: bad"] });
      expect(bar().textContent?.trim()).toBe("octocat · settings.toml, line 3: bad");
    });

    it("lists each file's problem", () => {
      const status = { state: "offline" } as const;
      render(StatusBar, { status, problems: ["machine.toml, line 1: bad", "settings.toml, line 3: bad"] });
      expect(bar().textContent?.trim()).toBe(
        "Offline: read-only · machine.toml, line 1: bad · settings.toml, line 3: bad",
      );
    });
  });
});

describe("StatusBar's line for a mirror that waits for an answer", () => {
  const online = { state: "online", login: "octocat" } as const;
  const line = () => screen.getByRole("button", { name: "python/cpython's mirror waits for your answer" });

  it("says so in the live region, even when the settings hide the bar, and offers no answers", async () => {
    render(StatusBar, { status: online, mirrorQuestion: "python/cpython", visible: false });
    expect(bar().textContent?.trim()).toBe("python/cpython's mirror waits for your answer");
    expect(bar().contains(line())).toBe(true);
    expect(screen.getAllByRole("button")).toEqual([line()]);
    // With nobody to ask the question, a click does nothing.
    await fireEvent.click(line());
    expect(line()).toBeTruthy();
  });

  it("follows the status and the problems", () => {
    render(StatusBar, {
      status: online,
      problems: ["settings.toml is broken"],
      mirrorQuestion: "python/cpython",
    });
    expect(bar().textContent?.replaceAll(/\s+/g, " ").trim()).toBe(
      "octocat · settings.toml is broken · python/cpython's mirror waits for your answer",
    );
  });

  it("asks the question at a click", async () => {
    const onquestion = vi.fn();
    render(StatusBar, { status: online, mirrorQuestion: "python/cpython", onquestion });
    await fireEvent.click(line());
    expect(onquestion).toHaveBeenCalledWith("python/cpython");
  });

  it("says nothing while no mirror waits", () => {
    render(StatusBar, { status: online });
    expect(bar().textContent).not.toContain("waits");
  });
});

describe("StatusBar's word on the mirror", () => {
  const online = { state: "online", login: "octocat" } as const;
  const short = {
    kind: "disk_short",
    repository: "python/cpython",
    folder: "D:\\Tsugiki",
    needed_mb: 6144,
    free_mb: 1024,
  } as const;

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("says nothing and offers nothing while a mirror can be made", () => {
    render(StatusBar, { status: online });
    expect(bar().textContent?.trim()).toBe("octocat");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("says what's short in the live region, and offers Check again outside it", async () => {
    const oncheckagain = vi.fn();
    render(StatusBar, { status: online, mirrorProblem: short, oncheckagain });
    expect(bar().textContent).toContain("No room to clone python/cpython: it needs 6.0 GB");
    expect(screen.queryByRole("button", { name: "Get git" })).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(oncheckagain).toHaveBeenCalledOnce();
  });

  it("has nothing to do at Check again unless the page says", async () => {
    render(StatusBar, { status: online, mirrorProblem: short });
    await fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(screen.getByRole("button", { name: "Check again" })).toBeTruthy();
  });

  it("comes back when the settings hide it, since nothing else says", () => {
    render(StatusBar, { status: online, mirrorProblem: short, visible: false });
    expect(bar().textContent?.trim()).toMatch(/^No room to clone/);
    expect(screen.getByRole("button", { name: "Check again" })).toBeTruthy();
  });

  it.each([{ kind: "no_git" }, { kind: "old_git", version: "git version 2.25.1" }] as const)(
    "opens git's downloads in the browser for %o",
    async (mirrorProblem) => {
      vi.mocked(openLink).mockResolvedValue(undefined);
      render(StatusBar, { status: online, mirrorProblem });
      await fireEvent.click(screen.getByRole("button", { name: "Get git" }));
      expect(openLink).toHaveBeenCalledWith("https://git-scm.com/downloads");
      expect(bar().textContent).toContain("git 2.30 or newer");
    },
  );

  it("says when the browser didn't open", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    render(StatusBar, { status: online, mirrorProblem: { kind: "no_git" } });
    await fireEvent.click(screen.getByRole("button", { name: "Get git" }));
    expect(bar().textContent).toContain("can't find git · Error: no browser");
  });

  it("drops the browser's problem once git is found", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    const { rerender } = render(StatusBar, { status: online, mirrorProblem: { kind: "no_git" } });
    await fireEvent.click(screen.getByRole("button", { name: "Get git" }));
    await rerender({ mirrorProblem: null });
    expect(bar().textContent?.trim()).toBe("octocat");
  });
});

describe("StatusBar's new version", () => {
  const online = { state: "online", login: "octocat" } as const;
  const newVersion = {
    version: "0.3.0",
    page: "https://github.com/tsugiki/tsugiki/releases/tag/v0.3.0",
  };
  const release = () => screen.getByRole("button", { name: "Tsugiki 0.3.0 is out" });

  it("says nothing until a check finds one", () => {
    render(StatusBar, { status: online });
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("opens the release page in the browser at a click", async () => {
    vi.mocked(openLink).mockResolvedValue(undefined);
    render(StatusBar, { status: online, newVersion });
    await fireEvent.click(release());
    expect(openLink).toHaveBeenCalledWith(newVersion.page);
    expect(bar().textContent?.trim()).toBe("octocat");
  });

  it("says when the browser didn't open", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    render(StatusBar, { status: online, newVersion });
    await fireEvent.click(release());
    expect(bar().textContent).toContain("octocat · Error: no browser");
  });

  it("drops the browser's problem when the bar hides the button", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    const { rerender } = render(StatusBar, { status: online, newVersion });
    await fireEvent.click(release());
    await rerender({ visible: false });
    expect(bar().textContent?.trim()).toBe("");
  });

  it("hides with the bar", () => {
    render(StatusBar, { status: online, newVersion, visible: false });
    expect(screen.queryByRole("button")).toBeNull();
  });
});

describe("StatusBar's sync under way", () => {
  const online = { state: "online", login: "octocat" } as const;
  const repository = "python/cpython";
  const lastSync = { repository, at: null };
  type Load = { issues_read: number; issues_total: number | null; prs_fetched: number; prs_known: number };
  const activity = (
    part: "your_work" | "window" | "trees" | "older_open" | "refresh",
    pages = 12,
    load: Load | null = null,
  ) => ({
    open: { repository, part, pages, load },
    paused_until: null,
    elsewhere: [] as string[],
  });

  it.each([
    ["your_work", "Syncing your work · 12 pages"],
    ["window", "Syncing recent items · 12 pages"],
    ["trees", "Syncing Issue trees · 12 pages"],
    ["older_open", "Syncing older open items · 12 pages"],
  ] as const)("says the part %s and the pages, outside the live region", (part, words) => {
    render(StatusBar, { status: online, lastSync, syncActivity: activity(part) });
    expect(screen.getByRole("button").textContent).toBe(words);
    expect(bar().textContent).not.toContain("Syncing");
  });

  it("counts no pages for the tree of an Issue the user opened", () => {
    render(StatusBar, { status: online, lastSync, syncActivity: activity("trees", 0) });
    expect(screen.getByRole("button").textContent).toBe("Syncing Issue trees");
  });

  it("says how far the queue has got, and that more PRs may come", () => {
    const load = { issues_read: 450, issues_total: 1446, prs_fetched: 900, prs_known: 2100 };
    render(StatusBar, { status: online, lastSync, syncActivity: activity("window", 12, load) });
    expect(screen.getByRole("button").textContent).toBe(
      "Syncing recent items · Issues 450 of 1446 · PRs 900 of 2100 so far",
    );
  });

  it("leaves out the total of Issues until GitHub has said it", () => {
    const load = { issues_read: 0, issues_total: null, prs_fetched: 12, prs_known: 40 };
    render(StatusBar, { status: online, lastSync, syncActivity: activity("trees", 12, load) });
    expect(screen.getByRole("button").textContent).toBe(
      "Syncing Issue trees · Issues 0 · PRs 12 of 40 so far",
    );
  });

  it("counts one page as one", () => {
    render(StatusBar, { status: online, lastSync, syncActivity: activity("window", 1) });
    expect(screen.getByRole("button").textContent).toBe("Syncing recent items · 1 page");
  });

  it("keeps the open Repository's sync time while another Repository syncs, and names it", () => {
    const other = { open: null, paused_until: null, elsewhere: ["python/peps"] };
    render(StatusBar, { status: online, lastSync, syncActivity: other });
    expect(screen.getByRole("button").textContent).toBe("Not synced yet · python/peps syncing");
    expect(bar().textContent).not.toContain("python/peps");
  });

  it("names the other Repository syncing beside the open one's sync", () => {
    const both = { ...activity("window"), elsewhere: ["python/peps"] };
    render(StatusBar, { status: online, lastSync, syncActivity: both });
    expect(screen.getByRole("button").textContent).toBe(
      "Syncing recent items · 12 pages · python/peps syncing",
    );
  });

  it("says nothing of a refresh", () => {
    render(StatusBar, { status: online, lastSync, syncActivity: activity("refresh") });
    expect(screen.getByRole("button").textContent).toBe("Not synced yet");
  });

  it("says in the live region when a sync waits for GitHub's points, even a refresh", () => {
    const pausedUntil = "2026-11-05T14:30:00Z";
    const time = new Date(pausedUntil).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
    render(StatusBar, {
      status: online,
      lastSync,
      syncActivity: { ...activity("refresh"), paused_until: pausedUntil },
    });
    expect(bar().textContent).toContain(`Sync paused until ${time}: GitHub's rate limit is low`);
  });

  it("shows the wait beside the file's problems, and when the bar is hidden", () => {
    render(StatusBar, {
      status: online,
      visible: false,
      problems: ["settings.toml has an error"],
      syncActivity: { ...activity("window"), paused_until: "2026-11-05T14:30:00Z" },
    });
    expect(bar().textContent).toContain("settings.toml has an error · Sync paused until");
  });

  it("shows a mirror's clone beside the sync time, outside the live region", () => {
    vi.useFakeTimers({ now: new Date("2026-11-05T12:02:00Z") });
    const cloning = { repository, progress: { stage: "receiving", percent: 42 } } as const;
    render(StatusBar, { status: online, lastSync: { repository, at: "2026-11-05T12:00:00Z" }, cloning });
    expect(screen.getByRole("button").textContent).toBe("Synced 2 min ago · Cloning python/cpython 42%");
    expect(bar().textContent).not.toContain("Cloning");
  });

  it("says a clone waits for the network while none is under way", () => {
    const offline = { state: "offline" } as const;
    const mirrorStates = [
      { repository: "python/peps", state: { state: "ready" } },
      { repository, state: { state: "offline" } },
    ] as const;
    render(StatusBar, { status: offline, lastSync, mirrorStates: [...mirrorStates] });
    expect(screen.getByRole("button").textContent).toBe(
      "Not synced yet · Cloning python/cpython starts once GitHub answers",
    );
  });

  it("puts a clone before the other Repositories syncing", () => {
    const cloning = { repository: "python/peps", progress: { stage: "starting" } } as const;
    const other = { open: null, paused_until: null, elsewhere: ["python/peps"] };
    render(StatusBar, { status: online, lastSync, syncActivity: other, cloning });
    expect(screen.getByRole("button").textContent).toBe(
      "Not synced yet · Cloning python/peps · python/peps syncing",
    );
  });
});

describe("StatusBar's chord", () => {
  const online = { state: "online", login: "octocat" } as const;

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows the first key on the right while Tsugiki waits for the second", () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    render(StatusBar, { status: online, chord: "ctrl+k" });
    const chord = screen.getByText("Ctrl+K");
    expect(bar().contains(chord)).toBe(false);
    expect(chord.parentElement).toBe(bar().parentElement);
  });

  it("names Cmd on macOS", () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    render(StatusBar, { status: online, chord: "ctrl+k" });
    expect(screen.getByText("Cmd+K")).toBeTruthy();
  });

  it("goes with the bar", () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    render(StatusBar, { status: online, chord: "ctrl+k", visible: false });
    expect(screen.queryByText("Ctrl+K")).toBeNull();
  });
});
