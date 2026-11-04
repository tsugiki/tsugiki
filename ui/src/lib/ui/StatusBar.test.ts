import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import StatusBar from "./StatusBar.svelte";

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
    expect(bar().classList.contains("beside-sync")).toBe(true);
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
