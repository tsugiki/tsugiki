import { fireEvent, render, screen, within } from "@testing-library/svelte";
import type { ComponentProps } from "svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openLink } from "$lib/state/browser";
import { type LoadedSettings, SettingsState, type Settings as Values } from "$lib/state/settings.svelte";
import Settings from "./Settings.svelte";

const core = vi.hoisted(() => ({ tokenSource: vi.fn(), rereadSettings: vi.fn(), applySettings: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({ commands: core }));
vi.mock("$lib/state/browser", () => ({ openLink: vi.fn() }));

const README = "https://github.com/tsugiki/tsugiki/blob/v0.3.1/crates/token/README.md";
const about = { name: "Tsugiki", version: "0.3.1", build: "release", source_ref: "v0.3.1" } as const;
const online = { state: "online", login: "octocat" } as const;

/** The defaults, with `change` made to them. */
const settingsWith = (change: (settings: Values) => void = () => {}): Values => {
  const settings: Values = {
    status_bar: { visible: true },
    releases: { check: true },
    appearance: { theme: "system", light_theme: "VS Code Light Modern", dark_theme: "VS Code Dark Modern" },
    diff: { files: "all", peek: "inline" },
    repositories: [
      { name: "some-maintainer/gcmon", conventions: "github", filters: [] },
      { name: "python/cpython", conventions: "cpython", filters: [] },
    ],
  };
  change(settings);
  return settings;
};
const loaded = (settings: Values, problems: string[] = []): LoadedSettings => ({
  settings,
  problems,
});
const ok = <T>(data: T) => ({ status: "ok", data }) as const;
const refused = (message: string) => ({ status: "error", error: { kind: "internal", message } }) as const;

/** The settings in force: the defaults, as the core read them at start. */
let state: SettingsState;
/** The dialog's props, with the status online and this release's About. */
const onreread = vi.fn();
const props = (onclose = () => {}): ComponentProps<typeof Settings> => ({
  settings: state,
  status: online,
  about,
  onreread,
  onclose,
});
const dialog = () => screen.getByRole("dialog", { name: "Settings" });
const nav = () => screen.getByRole("navigation", { name: "Settings pages" });
const go = (name: string) => fireEvent.click(within(nav()).getByRole("button", { name }));
const accountPage = () => screen.getByRole("region", { name: "Account" });
const statusBarBox = () => screen.getByRole("checkbox", { name: "Show the status bar" });
const releasesBox = () => screen.getByRole("checkbox", { name: "Check for new versions" });
const button = (name: string) => screen.getByRole("button", { name });
/** The dialog, once it has read the files again. */
const opened = async (onclose = () => {}) => {
  const rendered = render(Settings, props(onclose));
  await vi.waitFor(() => {
    expect(statusBarBox().hasAttribute("disabled")).toBe(false);
  });
  return rendered;
};

beforeEach(() => {
  vi.clearAllMocks();
  core.tokenSource.mockResolvedValue("gh");
  core.rereadSettings.mockResolvedValue(ok(loaded(settingsWith())));
  core.applySettings.mockImplementation(async (changes: Partial<Values>) =>
    ok(loaded({ ...settingsWith(), ...changes })),
  );
  state = new SettingsState();
  state.take(loaded(settingsWith()));
});

describe("the Settings dialog", () => {
  it("lists the six pages and a page for each Repository under Repositories, and opens on General", () => {
    render(Settings, props());
    const entries = Array.from(nav().querySelectorAll("button"));
    expect(entries.map((entry) => entry.textContent)).toEqual([
      "General",
      "Account",
      "Repositories",
      "gcmon",
      "cpython",
      "Sync",
      "Keyboard",
      "Advanced",
    ]);
    expect(entries[3]?.getAttribute("title")).toBe("some-maintainer/gcmon");
    expect(entries[0]?.getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("region", { name: "General" })).toBeTruthy();
  });

  it("lists a Repository written twice in settings.toml twice", () => {
    state.take(
      loaded(
        settingsWith((settings) => {
          const cpython = () => ({ name: "python/cpython", conventions: "cpython" as const, filters: [] });
          settings.repositories = [cpython(), cpython()];
        }),
      ),
    );
    render(Settings, props());
    expect(within(nav()).getAllByRole("button", { name: "python/cpython" })).toHaveLength(2);
  });

  it("shows the page picked in its left column", async () => {
    render(Settings, props());
    for (const page of ["Repositories", "Sync", "Keyboard", "Advanced"]) {
      await go(page);
      expect(within(nav()).getByRole("button", { name: page }).getAttribute("aria-current")).toBe("page");
      expect(
        within(screen.getByRole("region", { name: page })).getByText("Nothing to set here yet."),
      ).toBeTruthy();
    }

    await go("python/cpython");
    const repository = screen.getByRole("region", { name: "python/cpython" });
    expect(within(repository).getByRole("heading", { name: "python/cpython" })).toBeTruthy();
    expect(within(nav()).getByRole("button", { name: "python/cpython" }).getAttribute("aria-current")).toBe(
      "page",
    );
    expect(
      within(nav()).getByRole("button", { name: "Repositories" }).getAttribute("aria-current"),
    ).toBeNull();

    await go("Account");
    expect(accountPage()).toBeTruthy();
  });

  it("takes the focus, and gives it back when it goes", () => {
    const before = document.createElement("button");
    document.body.append(before);
    before.focus();
    const { unmount } = render(Settings, props());
    expect(document.activeElement).toBe(dialog());

    unmount();
    expect(document.activeElement).toBe(before);
    before.remove();
  });

  it("keeps Tab inside, from its last control round to ✕", async () => {
    render(Settings, props());
    button("OK").focus();
    expect(await fireEvent.keyDown(document.activeElement as Element, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(button("Close"));
  });

  it("reads both files again as it opens, so a hand edit made since start-up shows in it", async () => {
    core.rereadSettings.mockResolvedValue(
      ok(
        loaded(
          settingsWith((settings) => {
            settings.releases.check = false;
          }),
          ["settings.toml, line 9: expected a value"],
        ),
      ),
    );
    render(Settings, props());
    expect(statusBarBox().hasAttribute("disabled")).toBe(true);

    await vi.waitFor(() => {
      expect(releasesBox().hasAttribute("disabled")).toBe(false);
    });
    expect(releasesBox()).toHaveProperty("checked", false);
    expect(statusBarBox()).toHaveProperty("checked", true);
    expect(core.rereadSettings).toHaveBeenCalledOnce();
    // The rest of the window, such as the theme, takes what the files say now too.
    expect(onreread).toHaveBeenCalledOnce();
    expect(state.settings?.releases.check).toBe(false);
    expect(state.problems).toEqual(["settings.toml, line 9: expected a value"]);
    // What the files said as it opened is what Cancel would put back, so nothing has changed yet.
    expect(button("OK")).toBeTruthy();
  });

  it("says why it couldn't read the files again, keeps its controls off, and closes on OK", async () => {
    core.rereadSettings.mockResolvedValue(refused("Tsugiki stopped saving its settings."));
    const onclose = vi.fn();
    render(Settings, props(onclose));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Tsugiki can't read the settings files again: Tsugiki stopped saving its settings.",
    );
    expect(statusBarBox().hasAttribute("disabled")).toBe(true);
    expect(onreread).not.toHaveBeenCalled();

    await fireEvent.click(button("OK"));
    expect(onclose).toHaveBeenCalledOnce();
    expect(core.applySettings).not.toHaveBeenCalled();
    expect(state.settings).toEqual(settingsWith());
  });

  it("closes on OK with nothing changed, and saves nothing", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(button("OK"));
    expect(onclose).toHaveBeenCalledOnce();
    expect(core.applySettings).not.toHaveBeenCalled();
  });

  it("closes on OK before it has read the files", async () => {
    core.rereadSettings.mockReturnValue(new Promise(() => {}));
    const onclose = vi.fn();
    render(Settings, props(onclose));
    await fireEvent.click(button("OK"));
    expect(onclose).toHaveBeenCalledOnce();
    expect(core.applySettings).not.toHaveBeenCalled();
  });

  it("closes on Esc, on ✕ and on a press outside, but not on a press inside", async () => {
    const onclose = vi.fn();
    render(Settings, props(onclose));

    await fireEvent.keyDown(window, { key: "Enter" });
    await fireEvent.mouseDown(dialog());
    expect(onclose).not.toHaveBeenCalled();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onclose).toHaveBeenCalledTimes(1);
    await fireEvent.click(button("Close"));
    expect(onclose).toHaveBeenCalledTimes(2);
    // A browser would move the focus to the body after the dialog gave it back, if not stopped.
    expect(await fireEvent.mouseDown(screen.getByTestId("settings-scrim"))).toBe(false);
    expect(onclose).toHaveBeenCalledTimes(3);
  });
});

describe("Settings › General", () => {
  it("previews a change on the settings in force at once, and the button reads Apply", async () => {
    await opened();
    await fireEvent.click(statusBarBox());
    expect(state.statusBarVisible).toBe(false);
    expect(button("Apply")).toBeTruthy();

    await fireEvent.click(statusBarBox());
    expect(state.statusBarVisible).toBe(true);
    expect(button("OK")).toBeTruthy();
  });

  it("keeps a change on Apply, through the core's save", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(statusBarBox());
    await fireEvent.click(releasesBox());
    await fireEvent.click(button("Apply"));

    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(core.applySettings).toHaveBeenCalledWith({
      status_bar: { visible: false },
      releases: { check: false },
    });
    expect(state.statusBarVisible).toBe(false);
    expect(state.settings?.releases.check).toBe(false);
  });

  it("sends Apply only the tables it changed, so a hand edit to the others stays", async () => {
    await opened();
    await fireEvent.click(releasesBox());
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenCalledWith({ releases: { check: false } });
  });

  for (const [way, cancel] of [
    ["Cancel", () => fireEvent.click(button("Cancel"))],
    ["Esc", () => fireEvent.keyDown(window, { key: "Escape" })],
    ["✕", () => fireEvent.click(button("Close"))],
  ] as const) {
    it(`puts every change back on ${way}, and saves nothing`, async () => {
      const onclose = vi.fn();
      await opened(onclose);
      await fireEvent.click(statusBarBox());
      await fireEvent.click(releasesBox());
      expect(state.statusBarVisible).toBe(false);

      await cancel();
      expect(onclose).toHaveBeenCalledOnce();
      expect(state.settings).toEqual(settingsWith());
      expect(state.statusBarVisible).toBe(true);
      expect(core.applySettings).not.toHaveBeenCalled();
    });
  }

  it("stays open with why when the core refuses Apply's save, and Cancel still puts the change back", async () => {
    core.applySettings.mockResolvedValue(refused("settings.toml, line 1: expected `]`"));
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(statusBarBox());
    await fireEvent.click(button("Apply"));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Tsugiki can't save the settings: settings.toml, line 1: expected `]`",
    );
    expect(onclose).not.toHaveBeenCalled();
    expect(state.statusBarVisible).toBe(false);

    await fireEvent.click(button("Cancel"));
    expect(onclose).toHaveBeenCalledOnce();
    expect(state.statusBarVisible).toBe(true);
  });

  it("waits for Apply's save before Esc or another Apply can do anything", async () => {
    const saving = Promise.withResolvers<ReturnType<typeof ok<LoadedSettings>>>();
    core.applySettings.mockReturnValue(saving.promise);
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(statusBarBox());
    await fireEvent.click(button("Apply"));
    expect(button("Apply").hasAttribute("disabled")).toBe(true);
    expect(button("Cancel").hasAttribute("disabled")).toBe(true);

    await fireEvent.keyDown(window, { key: "Escape" });
    // A press before the button turned disabled, as a fast double click could make.
    button("Apply").removeAttribute("disabled");
    await fireEvent.click(button("Apply"));
    expect(onclose).not.toHaveBeenCalled();
    expect(core.applySettings).toHaveBeenCalledOnce();

    saving.resolve(ok(loaded(settingsWith((settings) => (settings.status_bar.visible = false)))));
    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(state.statusBarVisible).toBe(false);
  });

  it("says where each setting is stored", async () => {
    await opened();
    const page = screen.getByRole("region", { name: "General" });
    const stored = Array.from(page.querySelectorAll<HTMLElement>(".stored")).map((label) => [
      label.textContent,
      label.title,
    ]);
    expect(stored).toEqual([
      ["settings file", "Saved in settings.toml, which can travel between machines"],
      ["settings file", "Saved in settings.toml, which can travel between machines"],
    ]);
  });

  it("says Check for new versions takes effect at the next start", async () => {
    await opened();
    const row = releasesBox().closest(".row") as HTMLElement;
    expect(within(row).getByText("Takes effect at the next start")).toBeTruthy();
    const statusRow = statusBarBox().closest(".row") as HTMLElement;
    expect(within(statusRow).queryByText("Takes effect at the next start")).toBeNull();
  });
});

describe("Settings › Account", () => {
  /** The value beside `label`. */
  const value = (label: string) =>
    screen.getByText(label, { selector: "dt" }).nextElementSibling?.textContent.replace(/\s+/g, " ").trim();
  /** The dialog on its Account page. */
  const onAccount = async (given = props()) => {
    const rendered = render(Settings, given);
    await go("Account");
    return rendered;
  };

  it("shows who is signed in and that the token comes from gh", async () => {
    await onAccount();
    expect(value("Signed in as")).toBe("octocat");
    expect(await screen.findByText("gh", { selector: "b" })).toBeTruthy();
    expect(value("Token from")).toBe("gh Your token comes from the GitHub CLI (gh auth token).");
    expect(core.tokenSource).toHaveBeenCalledOnce();
  });

  it("says when the login is last time's, and why it names nobody", async () => {
    const { rerender } = await onAccount({
      ...props(),
      status: { state: "offline_as", last_login: "octocat" },
    });
    expect(value("Signed in as")).toBe("octocat Signed in last time. GitHub can't be reached now.");

    await rerender({ ...props(), status: undefined });
    expect(value("Signed in as")).toBe("Not known yet: GitHub hasn't answered.");
  });

  it("says nothing of the token's source until the core answers, and says why if it can't", async () => {
    const asking = Promise.withResolvers<string>();
    core.tokenSource.mockReturnValue(asking.promise);
    await onAccount();
    expect(value("Token from")).toBe("");
    asking.reject(new Error("no IPC outside Tauri"));
    expect(await screen.findByText("Tsugiki can't say: Error: no IPC outside Tauri")).toBeTruthy();
  });

  it("says what Tsugiki does with the token", async () => {
    await onAccount();
    expect(screen.getByRole("heading", { name: "What Tsugiki does with the token" })).toBeTruthy();
    const said = Array.from(accountPage().querySelectorAll("li")).map((line) =>
      line.textContent.replace(/\s+/g, " ").trim(),
    );
    expect(said).toEqual([
      "Only Tsugiki's core, written in Rust, holds it. The window shows PRs and comments that anyone can " +
        "write, so it never gets the token and can't reach the network.",
      "It goes only to api.github.com, with Tsugiki's requests to GitHub's API.",
      "It is never written to the settings files, the database or the log.",
    ]);
  });

  it("opens the token crate's README at this release's tag in the browser, on a click or a middle click", async () => {
    vi.mocked(openLink).mockResolvedValue(undefined);
    await onAccount();
    const link = screen.getByRole("link", { name: "README" });
    expect(link.getAttribute("href")).toBe(README);

    // The webview would follow the link itself, or open a window for it, if not stopped.
    expect(await fireEvent.click(link)).toBe(false);
    const auxclick = (button: number) =>
      fireEvent(link, new MouseEvent("auxclick", { button, bubbles: true, cancelable: true }));
    expect(await auxclick(1)).toBe(false);
    expect(openLink).toHaveBeenCalledTimes(2);
    expect(openLink).toHaveBeenCalledWith(README);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("leaves the right button and the mouse's back and forward buttons alone on the README", async () => {
    await onAccount();
    const link = screen.getByRole("link", { name: "README" });
    for (const button of [2, 3, 4]) {
      await fireEvent(link, new MouseEvent("auxclick", { button, bubbles: true, cancelable: true }));
    }
    expect(openLink).not.toHaveBeenCalled();
  });

  it("says why the browser didn't open", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    await onAccount();
    await fireEvent.click(screen.getByRole("link", { name: "README" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Error: no browser");
  });
});
