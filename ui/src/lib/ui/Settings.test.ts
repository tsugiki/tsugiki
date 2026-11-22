import { fireEvent, render, screen, within } from "@testing-library/svelte";
import type { ComponentProps } from "svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openLink } from "$lib/state/browser";
import type { ServerRow } from "$lib/state/language-servers.svelte";
import {
  type LanguageServerSettings,
  type LoadedSettings,
  SettingsState,
  type Settings as Values,
} from "$lib/state/settings.svelte";
import { type DarkQuery, ThemeState, type Themes, type ThemeView } from "$lib/state/theme.svelte";
import { defaultSettings, NO_SERVERS, ok, refused, repositoryCheck } from "$lib/testing/fixtures";
import { nextTask } from "$lib/testing/helpers";
import Settings from "./Settings.svelte";

const core = vi.hoisted(() => ({
  tokenSource: vi.fn(),
  rereadSettings: vi.fn(),
  applySettings: vi.fn(),
  languageServers: vi.fn(),
  pickProgram: vi.fn(),
  openServerLog: vi.fn(),
  themes: vi.fn(),
  previewThemes: vi.fn(),
  turnWindow: vi.fn(),
  onThisMachine: vi.fn(),
  settings: vi.fn(),
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  repositoryState: vi.fn(),
  repositorySync: vi.fn(),
}));
/** The listener for the servers' rows, so a test can send it what the core would, and its stop. */
const rowsHeard = vi.hoisted(() => ({
  send: (payload: unknown) => {
    void payload;
  },
  stop: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: core,
  events: {
    languageServersChanged: {
      listen: async (listener: (event: { payload: unknown }) => void) => {
        rowsHeard.send = (payload) => {
          listener({ payload });
        };
        return rowsHeard.stop;
      },
    },
    // A Repository's page listens for what can change its header.
    changed: { listen: async () => () => {} },
    statusChanged: { listen: async () => () => {} },
    repositorySyncChanged: { listen: async () => () => {} },
  },
}));
vi.mock("$lib/state/browser", () => ({ openLink: vi.fn() }));

const README = "https://github.com/tsugiki/tsugiki/blob/v0.3.1/crates/token/README.md";
const about = { name: "Tsugiki", version: "0.3.1", build: "release", source_ref: "v0.3.1" } as const;
const online = { state: "online", login: "octocat" } as const;

/** The defaults, with `change` made to them. */
const settingsWith = (change: (settings: Values) => void = () => {}): Values => {
  const settings: Values = defaultSettings({
    repositories: [
      { name: "some-maintainer/gcmon", conventions: "github", filters: [] },
      { name: "python/cpython", conventions: "cpython", filters: [] },
    ],
  });
  change(settings);
  return settings;
};
const loaded = (
  settings: Values,
  problems: string[] = [],
  language_servers: LanguageServerSettings = NO_SERVERS,
): LoadedSettings => ({
  settings,
  language_servers,
  problems,
});
/** The servers' rows as the core holds them unless a test says otherwise: ty ready with the version we
 * tested, clangd stopped on its own, and rust-analyzer not set up. */
const ROWS: ServerRow[] = [
  {
    server: "ty",
    status: { state: "ready" },
    version: { version: "0.0.103", tested: true },
    tested: ["0.0.103"],
  },
  {
    server: "clangd",
    status: {
      state: "problem",
      problem: { kind: "stopped", code: 139, stderr_tail: "I[12:00:01] Indexing\nclangd: out of memory\n" },
    },
    version: { version: "23.1.0", tested: true },
    tested: ["23.1.0"],
  },
  { server: "rust-analyzer", status: { state: "not_set_up" }, version: null, tested: ["1.99.1"] },
];

/** A theme of side `kind` named `name`, with one token. */
const themeView = (name: string, kind: "light" | "dark"): ThemeView => ({
  name,
  kind,
  tokens: [{ property: "--bg", value: kind === "light" ? "#ffffff" : "#1f1f1f" }],
});
/** The built-in themes, following the system, with a light theme file that can't be used. */
const THEMES: Themes = {
  theme: { kind: "system" },
  light: {
    chosen: "VS Code Light Modern",
    all: [themeView("VS Code Light Modern", "light"), themeView("Gruvbox Light Hard", "light")],
    failed: [
      { name: "Broken Light", file: "broken.toml", reason: "it extends Nothing, which no theme is named" },
    ],
  },
  dark: {
    chosen: "VS Code Dark Modern",
    all: [themeView("VS Code Dark Modern", "dark"), themeView("Gruvbox Dark Hard", "dark")],
    failed: [],
  },
  problems: [],
};
/** Windows on its light side, which says nothing when it changes. */
const LIGHT_SIDE = { matches: false, addEventListener: () => {} } as unknown as DarkQuery;
/** The themes the core works out for `appearance`, as `preview_themes` does for names the themes have: a
 * theme the themes hold is named, and anything else follows the system. */
const previewOf = (appearance: Values["appearance"]) => {
  const themes = structuredClone(THEMES);
  themes.light.chosen = appearance.light_theme;
  themes.dark.chosen = appearance.dark_theme;
  const side = (["light", "dark"] as const).find((kind) =>
    themes[kind].all.some((one) => one.name === appearance.theme),
  );
  themes.theme = side ? { kind: "named", name: appearance.theme, side } : { kind: "system" };
  return ok(themes);
};

/** The settings in force: the defaults, as the core read them at start. */
let state: SettingsState;
/** The window's themes, as the core sent them at start. */
let theme: ThemeState;
/** The dialog's props, with the status online and this release's About. */
const onreread = vi.fn();
const onadded = vi.fn();
const onswitch = vi.fn();
const props = (onclose = () => {}): ComponentProps<typeof Settings> => ({
  settings: state,
  theme,
  status: online,
  about,
  onreread,
  onadded,
  onswitch,
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

beforeEach(async () => {
  vi.clearAllMocks();
  core.tokenSource.mockResolvedValue("gh");
  core.rereadSettings.mockResolvedValue(ok(loaded(settingsWith())));
  core.applySettings.mockImplementation(
    async ({
      language_servers,
      ...changes
    }: Partial<Values> & { language_servers?: LanguageServerSettings }) =>
      ok(loaded({ ...settingsWith(), ...changes }, [], language_servers)),
  );
  core.languageServers.mockResolvedValue(ROWS);
  core.themes.mockImplementation(async () => ok(structuredClone(THEMES)));
  core.previewThemes.mockImplementation(async (appearance: Values["appearance"]) => previewOf(appearance));
  core.turnWindow.mockResolvedValue(null);
  core.onThisMachine.mockResolvedValue(ok([]));
  core.repositoryState.mockResolvedValue(ok({ read_only: [], connection: "online" }));
  core.repositorySync.mockResolvedValue(ok({ last_sync: null, open: null }));
  state = new SettingsState();
  state.take(loaded(settingsWith()));
  theme = new ThemeState(LIGHT_SIDE);
  await theme.load();
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
    await go("Sync");
    expect(within(nav()).getByRole("button", { name: "Sync" }).getAttribute("aria-current")).toBe("page");
    expect(
      within(screen.getByRole("region", { name: "Sync" })).getByText("Nothing to set here yet."),
    ).toBeTruthy();

    await go("Repositories");
    const repositories = screen.getByRole("region", { name: "Repositories" });
    expect(within(repositories).getByRole("button", { name: "Add repository…" })).toBeTruthy();

    await go("Keyboard");
    const keyboard = screen.getByRole("region", { name: "Keyboard" });
    expect(within(keyboard).getByRole("searchbox", { name: "Filter keys" })).toBeTruthy();

    await go("python/cpython");
    const repository = screen.getByRole("region", { name: "python/cpython" });
    expect(within(repository).getByRole("heading", { name: "python/cpython" })).toBeTruthy();
    expect(await within(repository).findByText("Can write · Not synced yet")).toBeTruthy();
    expect(core.repositorySync).toHaveBeenCalledWith("python/cpython");
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

  it("stays open on an Esc that clears Keyboard's filter box, and closes on the next", async () => {
    const onclose = vi.fn();
    render(Settings, props(onclose));
    await go("Keyboard");
    const box = screen.getByRole("searchbox", { name: "Filter keys" });
    await fireEvent.input(box, { target: { value: "archive" } });

    await fireEvent.keyDown(box, { key: "Escape" });
    expect([box, onclose.mock.calls.length]).toEqual([expect.objectContaining({ value: "" }), 0]);
    await fireEvent.keyDown(box, { key: "Escape" });
    expect(onclose).toHaveBeenCalledOnce();
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
    const file = ["settings file", "Saved in settings.toml, which can travel between machines"];
    expect(stored).toEqual([file, file, file, file, file]);
  });

  it("says Check for new versions takes effect at the next start", async () => {
    await opened();
    const row = releasesBox().closest(".row") as HTMLElement;
    expect(within(row).getByText("Takes effect at the next start")).toBeTruthy();
    const statusRow = statusBarBox().closest(".row") as HTMLElement;
    expect(within(statusRow).queryByText("Takes effect at the next start")).toBeNull();
  });
});

describe("Settings › General's Appearance", () => {
  const select = (name: string) => screen.getByRole("combobox", { name }) as HTMLSelectElement;
  const chosen = (name: string) => select(name).selectedOptions[0]?.textContent;
  const choose = (name: string, value: string) => fireEvent.change(select(name), { target: { value } });
  /** The dialog, opened on a file whose `[appearance]` `change` makes. */
  const openedOn = (change: (settings: Values) => void) => {
    core.rereadSettings.mockResolvedValue(ok(loaded(settingsWith(change))));
    return opened();
  };

  it("comes first, and lists Follow system, then the light themes, then the dark ones, without the files that failed", async () => {
    await opened();
    const page = screen.getByRole("region", { name: "General" });
    expect(page.querySelector("h3")?.textContent).toBe("Appearance");
    const groups = Array.from(select("Theme").children).map((child) =>
      child instanceof HTMLOptGroupElement
        ? [child.label, Array.from(child.children).map((option) => option.textContent)]
        : child.textContent,
    );
    expect(groups).toEqual([
      "Follow system",
      ["Light", ["VS Code Light Modern", "Gruvbox Light Hard"]],
      ["Dark", ["VS Code Dark Modern", "Gruvbox Dark Hard"]],
    ]);
    const names = (row: string) => Array.from(select(row).options).map((option) => option.textContent);
    expect(names("Light theme")).toEqual(["VS Code Light Modern", "Gruvbox Light Hard"]);
    expect(names("Dark theme")).toEqual(["VS Code Dark Modern", "Gruvbox Dark Hard"]);
    expect(chosen("Theme")).toBe("Follow system");
    expect(chosen("Light theme")).toBe("VS Code Light Modern");
  });

  it("says under each side's row that Follow system shows it", async () => {
    await opened();
    const help = (name: string) =>
      (select(name).closest(".row") as HTMLElement).querySelector(".help")?.textContent;
    expect(help("Light theme")).toBe("Follow system shows it while the system is light.");
    expect(help("Dark theme")).toBe("Follow system shows it while the system is dark.");
    expect(help("Theme")).toBeUndefined();
  });

  it("shows a dark Theme on the window before Apply, and Cancel puts the Theme in force back", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    expect(theme.shown?.name).toBe("VS Code Light Modern");

    await choose("Theme", "Gruvbox Dark Hard");
    await vi.waitFor(() => {
      expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    });
    expect(core.previewThemes).toHaveBeenLastCalledWith({
      theme: "Gruvbox Dark Hard",
      light_theme: "VS Code Light Modern",
      dark_theme: "VS Code Dark Modern",
    });
    expect(core.turnWindow).toHaveBeenLastCalledWith("dark");
    expect(button("Apply")).toBeTruthy();

    await fireEvent.click(button("Cancel"));
    expect(onclose).toHaveBeenCalledOnce();
    await vi.waitFor(() => {
      expect(core.turnWindow).toHaveBeenLastCalledWith(null);
    });
    expect(theme.shown?.name).toBe("VS Code Light Modern");
    expect(state.settings?.appearance.theme).toBe("system");
    expect(core.applySettings).not.toHaveBeenCalled();
  });

  it("shows a side's theme on the window at once while Follow system shows that side", async () => {
    await opened();
    await choose("Dark theme", "Gruvbox Dark Hard");
    await vi.waitFor(() => {
      expect(core.previewThemes).toHaveBeenCalledOnce();
    });
    expect(theme.shown?.name).toBe("VS Code Light Modern");

    await choose("Light theme", "Gruvbox Light Hard");
    await vi.waitFor(() => {
      expect(theme.shown?.name).toBe("Gruvbox Light Hard");
    });
  });

  it("shows Follow system on the side the system is on, not the side a theme kept the window on", async () => {
    // The file names a dark theme, which keeps the window, and so the page's media query, dark.
    let heard: ((e: { matches: boolean }) => void) | undefined;
    const query = {
      matches: false,
      addEventListener: (_: string, listener: (e: { matches: boolean }) => void) => {
        heard = listener;
      },
    } as unknown as DarkQuery;
    const named = { kind: "named", name: "Gruvbox Dark Hard", side: "dark" } as const;
    core.themes.mockImplementation(async () => ok({ ...structuredClone(THEMES), theme: named }));
    theme = new ThemeState(query);
    await theme.load();
    heard?.({ matches: true });
    await openedOn((settings) => (settings.appearance.theme = "Gruvbox Dark Hard"));
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    // Turned back to the system, the window says the system is light.
    core.turnWindow.mockResolvedValue("light");

    await choose("Theme", "system");

    await vi.waitFor(() => {
      expect(theme.shown?.name).toBe("VS Code Light Modern");
    });
    expect(core.turnWindow).toHaveBeenLastCalledWith(null);
  });

  it("saves a changed Light theme on Apply and leaves theme alone, and keeps showing it", async () => {
    core.applySettings.mockResolvedValue(
      ok(loaded(settingsWith((settings) => (settings.appearance.light_theme = "Gruvbox Light Hard")))),
    );
    const onclose = vi.fn();
    await opened(onclose);
    const asked = core.themes.mock.calls.length;
    await choose("Light theme", "Gruvbox Light Hard");
    const saved = settingsWith((settings) => (settings.appearance.light_theme = "Gruvbox Light Hard"));
    core.themes.mockImplementation(async () => previewOf(saved.appearance));
    await fireEvent.click(button("Apply"));

    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(core.applySettings).toHaveBeenCalledWith({ appearance: { light_theme: "Gruvbox Light Hard" } });
    expect(theme.themes?.light.chosen).toBe("Gruvbox Light Hard");
    expect(theme.shown?.name).toBe("Gruvbox Light Hard");
    // It reads the themes again for the files' problems, with the line the file now gives.
    expect(core.themes.mock.calls.length).toBeGreaterThan(asked);
  });

  it("makes the theme it shows the theme in force on Apply, so a slow read of the themes doesn't take it back", async () => {
    core.applySettings.mockResolvedValue(
      ok(loaded(settingsWith((settings) => (settings.appearance.theme = "Gruvbox Dark Hard")))),
    );
    const onclose = vi.fn();
    await opened(onclose);
    await choose("Theme", "Gruvbox Dark Hard");
    await vi.waitFor(() => {
      expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    });
    const themes = Promise.withResolvers<ReturnType<typeof ok<Themes>>>();
    core.themes.mockReturnValue(themes.promise);

    await fireEvent.click(button("Apply"));
    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    expect(theme.themes?.theme).toEqual({ kind: "named", name: "Gruvbox Dark Hard", side: "dark" });

    themes.resolve(ok({ ...structuredClone(THEMES), problems: ["night.toml: name isn't a theme's name"] }));
    await vi.waitFor(() => {
      expect(theme.problems).toEqual(["night.toml: name isn't a theme's name"]);
    });
  });

  it("says why the window can't show a change, and stays open with why when Apply can't show it either", async () => {
    core.previewThemes.mockResolvedValue(refused("Tsugiki can't read themes: access denied"));
    core.applySettings.mockResolvedValue(
      ok(loaded(settingsWith((settings) => (settings.appearance.theme = "Gruvbox Dark Hard")))),
    );
    const onclose = vi.fn();
    await opened(onclose);
    await choose("Theme", "Gruvbox Dark Hard");
    const page = screen.getByRole("region", { name: "General" });
    expect(
      await within(page).findByText("Tsugiki can't show the theme: Tsugiki can't read themes: access denied"),
    ).toBeTruthy();
    expect(theme.shown?.name).toBe("VS Code Light Modern");
    core.themes.mockResolvedValue(refused("Tsugiki can't read themes: access denied"));

    await fireEvent.click(button("Apply"));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Tsugiki saved the settings, but can't show the theme: Tsugiki can't read themes: access denied",
    );
    expect(onclose).not.toHaveBeenCalled();
    expect(button("OK")).toBeTruthy();
  });

  it("turns the rows off while Apply saves, so the window keeps the themes of what it saved", async () => {
    const saving = Promise.withResolvers<ReturnType<typeof ok<LoadedSettings>>>();
    core.applySettings.mockReturnValue(saving.promise);
    const onclose = vi.fn();
    await opened(onclose);
    await choose("Theme", "Gruvbox Dark Hard");
    await vi.waitFor(() => {
      expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    });

    await fireEvent.click(button("Apply"));
    expect(select("Theme").disabled).toBe(true);
    expect(select("Light theme").disabled).toBe(true);
    expect(select("Dark theme").disabled).toBe(true);

    const saved = settingsWith((settings) => (settings.appearance.theme = "Gruvbox Dark Hard"));
    core.themes.mockImplementation(async () => previewOf(saved.appearance));
    saving.resolve(ok(loaded(saved)));
    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(theme.themes?.theme).toEqual({ kind: "named", name: "Gruvbox Dark Hard", side: "dark" });
  });

  it("lets Cancel close after an Apply whose call throws", async () => {
    core.applySettings.mockImplementation(() => {
      throw new Error("no IPC");
    });
    const onclose = vi.fn();
    await opened(onclose);
    await choose("Theme", "Gruvbox Dark Hard");
    await fireEvent.click(button("Apply"));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Tsugiki can't save the settings: Error: no IPC",
    );
    await fireEvent.click(button("Cancel"));
    expect(onclose).toHaveBeenCalledOnce();
  });

  it("says why it can't offer the themes when the window never got them", async () => {
    core.themes.mockResolvedValue(refused("Tsugiki can't read themes: access denied"));
    theme = new ThemeState(LIGHT_SIDE);
    await theme.load();
    await opened();
    const page = screen.getByRole("region", { name: "General" });
    expect(
      within(page).getByText("Tsugiki can't list the themes: Tsugiki can't read themes: access denied"),
    ).toBeTruthy();
    expect(select("Theme").disabled).toBe(true);
  });

  it("saves Follow system and a Theme as the theme picker's picks", async () => {
    const first = await openedOn((settings) => (settings.appearance.theme = "Gruvbox Light Hard"));
    await choose("Theme", "system");
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenLastCalledWith({ appearance: { theme: { kind: "system" } } });
    first.unmount();

    await openedOn(() => {});
    await choose("Theme", "Gruvbox Dark Hard");
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenLastCalledWith({
      appearance: { theme: { kind: "named", name: "Gruvbox Dark Hard" } },
    });
  });

  it("shows a dark_theme no Theme has as not found, and Apply of another change keeps it", async () => {
    await openedOn((settings) => (settings.appearance.dark_theme = "Solarized Dark"));
    expect(chosen("Dark theme")).toBe("Solarized Dark (not found)");
    expect(Array.from(select("Dark theme").options).map((option) => option.textContent)).toEqual([
      "Solarized Dark (not found)",
      "VS Code Dark Modern",
      "Gruvbox Dark Hard",
    ]);

    await fireEvent.click(statusBarBox());
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenCalledWith({ status_bar: { visible: false } });
  });

  it("keeps offering what the file held after a change, so the user can go back to it", async () => {
    await openedOn((settings) => {
      settings.appearance.theme = "dark";
      settings.appearance.dark_theme = "Solarized Dark";
    });
    await choose("Theme", "Gruvbox Light Hard");
    await choose("Dark theme", "Gruvbox Dark Hard");
    expect(chosen("Theme")).toBe("Gruvbox Light Hard");
    expect(Array.from(select("Theme").options).map((option) => option.textContent)).toContain(
      "Always the Dark theme",
    );

    await choose("Theme", "dark");
    await choose("Dark theme", "Solarized Dark");
    expect(chosen("Theme")).toBe("Always the Dark theme");
    expect(chosen("Dark theme")).toBe("Solarized Dark (not found)");
    expect(button("OK")).toBeTruthy();
  });

  it("shows a theme no Theme has as not found, and a mode from before as its side, which Apply never sends", async () => {
    for (const [held, words] of [
      ["Solarized Dark", "Solarized Dark (not found)"],
      ["dark", "Always the Dark theme"],
    ] as const) {
      const rendered = await openedOn((settings) => (settings.appearance.theme = held));
      expect(chosen("Theme")).toBe(words);
      rendered.unmount();
    }

    await openedOn((settings) => (settings.appearance.theme = "light"));
    expect(chosen("Theme")).toBe("Always the Light theme");
    await choose("Light theme", "Gruvbox Light Hard");
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenCalledWith({ appearance: { light_theme: "Gruvbox Light Hard" } });
  });

  it("says what a name the file holds is when it names a theme of the other side or a file that failed", async () => {
    for (const [change, row, words] of [
      [
        (settings: Values) => (settings.appearance.light_theme = "Gruvbox Dark Hard"),
        "Light theme",
        "Gruvbox Dark Hard (a dark theme)",
      ],
      [
        (settings: Values) => (settings.appearance.dark_theme = "VS Code Light Modern"),
        "Dark theme",
        "VS Code Light Modern (a light theme)",
      ],
      [
        (settings: Values) => (settings.appearance.light_theme = "Broken Light"),
        "Light theme",
        "Broken Light (can't be used)",
      ],
      [
        (settings: Values) => (settings.appearance.theme = "Broken Light"),
        "Theme",
        "Broken Light (can't be used)",
      ],
    ] as const) {
      const rendered = await openedOn(change);
      expect(chosen(row), words).toBe(words);
      rendered.unmount();
    }
  });

  it("waits for the window's themes before it offers any", async () => {
    theme = new ThemeState(LIGHT_SIDE);
    await opened();
    expect(select("Theme").disabled).toBe(true);
    expect(select("Light theme").disabled).toBe(true);
    expect(Array.from(select("Dark theme").options).map((option) => option.textContent)).toEqual([
      "VS Code Dark Modern",
    ]);

    await theme.load();
    await vi.waitFor(() => {
      expect(select("Theme").disabled).toBe(false);
    });
    expect(chosen("Dark theme")).toBe("VS Code Dark Modern");
  });
});

describe("Settings › Advanced", () => {
  const page = () => screen.getByRole("region", { name: "Advanced" });
  const row = (name: string) => within(page()).getByRole("group", { name });
  const path = (name: string) => within(row(name)).getByRole("textbox") as HTMLInputElement;
  const idle = () => within(page()).getByRole("spinbutton") as HTMLInputElement;
  /** The words of each paragraph of `name`'s row. */
  const said = (name: string) => Array.from(row(name).querySelectorAll("p")).map((line) => line.textContent);
  /** The dialog on Settings › Advanced, once it has read the files, with `given` props. */
  const onAdvanced = async (given: Partial<ComponentProps<typeof Settings>> = {}) => {
    const rendered = render(Settings, { ...props(), ...given });
    await go("Advanced");
    await vi.waitFor(() => {
      expect(path("ty").hasAttribute("disabled")).toBe(false);
    });
    return rendered;
  };

  beforeEach(() => {
    core.rereadSettings.mockResolvedValue(
      ok(loaded(settingsWith(), [], { ...NO_SERVERS, ty: "tools/ty.exe", clangd: "C:/llvm/bin/clangd.exe" })),
    );
  });

  it("shows each server's path, the version found, the versions tested, its state and its last problem", async () => {
    await onAdvanced();
    expect(screen.getByRole("heading", { name: "Language servers" })).toBeTruthy();
    const names = within(page())
      .getAllByRole("group")
      .map((group) => group.getAttribute("aria-label"));
    expect(names).toEqual(["ty", "clangd", "rust-analyzer"]);
    expect([path("ty").value, path("clangd").value, path("rust-analyzer").value]).toEqual([
      "tools/ty.exe",
      "C:/llvm/bin/clangd.exe",
      "",
    ]);
    expect(said("ty")).toEqual(["Version 0.0.103, the one we tested", "Ready"]);
    expect(said("clangd")).toEqual(["Version 23.1.0, the one we tested", "clangd stopped (exit code 139)"]);
    expect(within(row("clangd")).getByLabelText("What clangd wrote last").textContent).toBe(
      "I[12:00:01] Indexing\nclangd: out of memory",
    );
    expect(said("rust-analyzer")).toEqual(["Tested with 1.99.1", "Not set up"]);
    expect(idle().value).toBe("30");
    // Each is kept in machine.toml.
    const stored = Array.from(page().querySelectorAll<HTMLElement>(".stored")).map((label) => label.title);
    expect(stored).toEqual(Array(4).fill("Saved in machine.toml, on this machine only"));
  });

  it("shows a row as the core says it changed, over the rows it asked for", async () => {
    await onAdvanced();
    rowsHeard.send([
      { ...ROWS[0], status: { state: "running" }, version: { version: "0.0.110", tested: false } },
      { ...ROWS[1], status: { state: "idle" } },
    ]);
    await vi.waitFor(() => {
      expect(said("ty")).toEqual(["Tested with 0.0.103; you have 0.0.110", "Running"]);
    });
    expect(said("clangd")).toEqual(["Version 23.1.0, the one we tested", "Stopped (idle)"]);
    expect(said("rust-analyzer")).toEqual([]);
  });

  it("saves a path typed or cleared on Apply, and stays open to show what the check finds", async () => {
    const onclose = vi.fn();
    await onAdvanced({ onclose });
    // As Explorer's "Copy as path" gives it.
    await fireEvent.change(path("rust-analyzer"), { target: { value: ' "C:/rust/bin/rust-analyzer.exe" ' } });
    await fireEvent.change(path("clangd"), { target: { value: "  " } });
    await fireEvent.click(button("Apply"));

    await vi.waitFor(() => {
      expect(button("OK")).toBeTruthy();
    });
    const servers = { ty: "tools/ty.exe", "rust-analyzer": "C:/rust/bin/rust-analyzer.exe", clangd: null };
    expect(core.applySettings).toHaveBeenCalledWith({ language_servers: { ...servers, idle_minutes: 30 } });
    expect(onclose).not.toHaveBeenCalled();
    expect(path("rust-analyzer").value).toBe("C:/rust/bin/rust-analyzer.exe");
    expect(path("clangd").value).toBe("");

    rowsHeard.send([
      { ...ROWS[2], status: { state: "ready" }, version: { version: "1.99.1", tested: true } },
    ]);
    await vi.waitFor(() => {
      expect(said("rust-analyzer")).toEqual(["Version 1.99.1, the one we tested", "Ready"]);
    });
    // Cancel now puts back what the Apply saved.
    await fireEvent.change(path("ty"), { target: { value: "elsewhere/ty.exe" } });
    await fireEvent.click(button("Cancel"));
    expect(onclose).toHaveBeenCalledOnce();
    expect(state.languageServers).toEqual({ ...servers, idle_minutes: 30 });
  });

  it("puts a changed path back on Cancel", async () => {
    await onAdvanced();
    await fireEvent.change(path("ty"), { target: { value: "elsewhere/ty.exe" } });
    expect(button("Apply")).toBeTruthy();
    await fireEvent.click(button("Cancel"));
    expect(state.languageServers?.ty).toBe("tools/ty.exe");
  });

  it("takes a whole number of minutes above 0 for the idle time, and shows the one in force for the rest", async () => {
    await onAdvanced();
    for (const typed of ["0", "2.5", ""]) {
      await fireEvent.change(idle(), { target: { value: typed } });
      expect(idle().value).toBe("30");
    }
    expect(button("OK")).toBeTruthy();

    await fireEvent.change(idle(), { target: { value: "10" } });
    await fireEvent.click(button("Apply"));
    expect(core.applySettings).toHaveBeenCalledWith({
      language_servers: {
        ...NO_SERVERS,
        ty: "tools/ty.exe",
        clangd: "C:/llvm/bin/clangd.exe",
        idle_minutes: 10,
      },
    });
  });

  it("fills in the path Browse… picked, and leaves it be when the user cancels the picker", async () => {
    core.pickProgram.mockResolvedValueOnce(ok("D:/tools/rust-analyzer.exe")).mockResolvedValueOnce(ok(null));
    await onAdvanced();
    await fireEvent.click(within(row("rust-analyzer")).getByRole("button", { name: "Browse…" }));
    await vi.waitFor(() => {
      expect(path("rust-analyzer").value).toBe("D:/tools/rust-analyzer.exe");
    });
    await fireEvent.click(within(row("rust-analyzer")).getByRole("button", { name: "Browse…" }));
    await vi.waitFor(() => {
      expect(core.pickProgram).toHaveBeenCalledTimes(2);
    });
    expect(path("rust-analyzer").value).toBe("D:/tools/rust-analyzer.exe");
  });

  it("says why Browse… couldn't open the picker", async () => {
    core.pickProgram.mockResolvedValue(refused("no dialog"));
    await onAdvanced();
    await fireEvent.click(within(row("ty")).getByRole("button", { name: "Browse…" }));
    expect((await within(row("ty")).findByRole("alert")).textContent).toBe(
      "Tsugiki can't open the file picker: no dialog",
    );
  });

  it("opens the log of the open Repository's server, and says why it couldn't", async () => {
    core.openServerLog.mockResolvedValueOnce(ok(null)).mockResolvedValueOnce(refused("no log yet"));
    await onAdvanced({ repository: "python/cpython" });
    const openLog = within(row("clangd")).getByRole("button", { name: "Open log" });
    expect(openLog.title).toBe("The log clangd writes for python/cpython");
    await fireEvent.click(openLog);
    await vi.waitFor(() => {
      expect(core.openServerLog).toHaveBeenCalledWith("python/cpython", "clangd");
    });
    expect(within(row("clangd")).queryByRole("alert")).toBeNull();

    await fireEvent.click(openLog);
    expect((await within(row("clangd")).findByRole("alert")).textContent).toBe(
      "Tsugiki can't open clangd's log: no log yet",
    );
  });

  it("offers no Open log without an open Repository", async () => {
    await onAdvanced();
    const openLog = within(row("ty")).getByRole("button", { name: "Open log" });
    expect(openLog.hasAttribute("disabled")).toBe(true);
    expect(openLog.title).toBe("Open a Repository to open the log its server writes");
  });

  it("opens on Advanced at a server's row, whose path takes the focus once the files are read", async () => {
    render(Settings, { ...props(), at: "clangd" });
    expect(within(nav()).getByRole("button", { name: "Advanced" }).getAttribute("aria-current")).toBe("page");
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(path("clangd"));
    });
    expect(row("clangd").classList.contains("asked")).toBe(true);
    expect(row("ty").classList.contains("asked")).toBe(false);
  });

  it("says why it can't say how the servers are, and stops listening when it goes", async () => {
    core.languageServers.mockRejectedValue(new Error("no IPC outside Tauri"));
    const { unmount } = await onAdvanced();
    expect((await within(page()).findByRole("alert")).textContent).toBe(
      "Tsugiki can't say how the language servers are: Error: no IPC outside Tauri",
    );
    expect(said("ty")).toEqual([]);
    unmount();
    await vi.waitFor(() => {
      expect(rowsHeard.stop).toHaveBeenCalledOnce();
    });
  });

  it("keeps its controls off until it has read the files", async () => {
    core.rereadSettings.mockReturnValue(new Promise(() => {}));
    render(Settings, props());
    await go("Advanced");
    expect(path("ty").hasAttribute("disabled")).toBe(true);
    expect(idle().hasAttribute("disabled")).toBe(true);
    await fireEvent.change(path("ty"), { target: { value: "ty.exe" } });
    expect(state.languageServers).toEqual(NO_SERVERS);
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
        "write, so it never gets the token, and reaches the network only for the images GitHub serves for them.",
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

describe("Settings › Repositories", () => {
  const repositoriesPage = () => screen.getByRole("region", { name: "Repositories" });
  /** Each Repository's row on the page: its name and its line. */
  const rows = () =>
    within(repositoriesPage())
      .queryAllByRole("listitem")
      .map((row) => Array.from(row.querySelectorAll("span:not([aria-hidden])"), (span) => span.textContent));
  const addButton = () => within(repositoriesPage()).getByRole("button", { name: "Add repository…" });
  const newproj = repositoryCheck("some-maintainer/newproj");
  /** What the window opens on once Add repository… has added some-maintainer/newproj. */
  const addedHome = {
    screen: "repository",
    repository: newproj.repository,
    others: ["some-maintainer/gcmon", "python/cpython"],
  } as const;
  const addDialog = () => screen.queryByRole("dialog", { name: "Add repository" });
  /** Opens Add repository… from the page. */
  const openAdd = async () => {
    await opened();
    await go("Repositories");
    await fireEvent.click(addButton());
  };

  it("shows each Repository with its Filters, its clone and its mirror's size, and a row opens its page", async () => {
    const filter = { name: "Mine", query: "author:@me", on_workbench: true };
    const files = settingsWith((settings) => {
      settings.repositories[1]?.filters.push(filter);
    });
    core.rereadSettings.mockResolvedValue(ok(loaded(files)));
    core.onThisMachine.mockResolvedValue(
      ok([
        { repository: "some-maintainer/gcmon", source: null, mirror: { kind: "missing" } },
        {
          repository: "python/cpython",
          source: { kind: "borrow", folder: "D:\\src\\cpython" },
          mirror: { kind: "measured", mb: 1536 },
        },
      ]),
    );
    await opened();

    await go("Repositories");

    await vi.waitFor(() => {
      expect(rows()).toEqual([
        ["some-maintainer/gcmon", "0 Filters · no mirror yet"],
        ["python/cpython", "1 Filter · borrows from a clone · mirror 1.5 GB"],
      ]);
    });
    await fireEvent.click(within(repositoriesPage()).getByRole("button", { name: /^python\/cpython/ }));
    expect(screen.getByRole("heading", { name: "python/cpython" })).toBeTruthy();
    expect(within(nav()).getByRole("button", { name: "python/cpython" }).getAttribute("aria-current")).toBe(
      "page",
    );
  });

  it("asks the core again each time the page shows", async () => {
    await opened();
    await go("Repositories");
    await go("General");
    await go("Repositories");
    expect(core.onThisMachine).toHaveBeenCalledTimes(2);
  });

  it("lists the Repositories without their mirrors when the core can't measure them, and says why", async () => {
    core.onThisMachine.mockResolvedValue(refused("Tsugiki can't find its folders."));
    await opened();

    await go("Repositories");

    expect(
      await within(repositoriesPage()).findByText(
        "Tsugiki can't measure the mirrors: Tsugiki can't find its folders.",
      ),
    ).toBeTruthy();
    expect(rows()).toEqual([
      ["some-maintainer/gcmon", "0 Filters"],
      ["python/cpython", "0 Filters"],
    ]);
  });

  it("offers only Add repository… with no Repositories", async () => {
    const none = settingsWith((settings) => {
      settings.repositories = [];
    });
    core.rereadSettings.mockResolvedValue(ok(loaded(none)));
    state.take(loaded(none));
    await opened();
    await go("Repositories");
    expect(within(repositoriesPage()).queryByRole("list")).toBeNull();
    expect(within(repositoriesPage()).getByRole("button", { name: "Add repository…" })).toBeTruthy();
  });

  it("adds a Repository with Add repository… over the dialog, lists it, and keeps it on Cancel", async () => {
    core.checkRepository.mockResolvedValue(ok({ kind: "new", check: newproj }));
    core.addRepository.mockResolvedValue(ok(addedHome));
    // The file had a problem as the dialog read it, which the user mended before the add read it again.
    const problem = "settings.toml, line 3: expected a value";
    core.rereadSettings.mockResolvedValue(ok(loaded(settingsWith(), [problem])));
    const withNewproj = settingsWith((settings) => {
      settings.repositories.push(newproj.repository);
    });
    core.settings.mockResolvedValue(loaded(withNewproj));
    await openAdd();
    const adding = addDialog();
    expect(adding).toBeTruthy();
    expect(document.activeElement).toBe(within(adding as HTMLElement).getByRole("textbox"));

    const input = screen.getByRole("textbox", { name: "Repository" });
    await fireEvent.input(input, { target: { value: "some-maintainer/newproj" } });
    await fireEvent.click(button("Check"));
    await fireEvent.click(await screen.findByRole("button", { name: "Add" }));

    await vi.waitFor(() => {
      expect(onadded).toHaveBeenCalledExactlyOnceWith(addedHome);
    });
    expect(addDialog()).toBeNull();
    expect(rows()).toEqual([
      ["some-maintainer/gcmon", "0 Filters"],
      ["python/cpython", "0 Filters"],
      ["some-maintainer/newproj", "0 Filters"],
    ]);
    expect(within(nav()).getByRole("button", { name: "some-maintainer/newproj" })).toBeTruthy();
    await vi.waitFor(() => {
      expect(core.onThisMachine).toHaveBeenCalledTimes(2);
    });
    expect(button("OK")).toBeTruthy();

    expect(state.problems).toEqual([problem]);

    await fireEvent.click(button("Cancel"));
    expect(state.settings?.repositories.map((repository) => repository.name)).toEqual([
      "some-maintainer/gcmon",
      "python/cpython",
      "some-maintainer/newproj",
    ]);
    await vi.waitFor(() => {
      expect(state.problems).toEqual([]);
    });
    expect(core.settings).toHaveBeenCalledOnce();
  });

  it("closes only Add repository… on Esc, and the dialog under it on the next", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    await go("Repositories");
    addButton().focus();
    await fireEvent.click(addButton());

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(addDialog()).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(addButton());

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onclose).toHaveBeenCalledOnce();
  });

  it("closes Add repository… with its Cancel, and leaves Settings open", async () => {
    await openAdd();
    await fireEvent.click(within(addDialog() as HTMLElement).getByRole("button", { name: "Cancel" }));
    expect(addDialog()).toBeNull();
    expect(dialog()).toBeTruthy();
  });

  it("closes as Cancel does on Switch to it, putting a change back, and hands on the name", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(statusBarBox());
    expect(state.statusBarVisible).toBe(false);
    await go("Repositories");
    core.checkRepository.mockResolvedValue(ok({ kind: "held", name: "python/cpython" }));
    await fireEvent.click(addButton());
    await fireEvent.click(button("Check"));

    await fireEvent.click(await screen.findByRole("button", { name: "Switch to it" }));

    expect(onclose).toHaveBeenCalledOnce();
    expect(onswitch).toHaveBeenCalledExactlyOnceWith("python/cpython");
    expect(onclose.mock.invocationCallOrder[0]).toBeLessThan(onswitch.mock.invocationCallOrder[0] ?? 0);
    expect(state.statusBarVisible).toBe(true);
    expect(addDialog()).toBeNull();
    expect(onadded).not.toHaveBeenCalled();
    expect(core.applySettings).not.toHaveBeenCalled();
  });

  it("stays open and switches nothing on Switch to it while Apply saves, and keeps Add repository… off", async () => {
    const onclose = vi.fn();
    await opened(onclose);
    await fireEvent.click(statusBarBox());
    await go("Repositories");
    core.checkRepository.mockResolvedValue(ok({ kind: "held", name: "python/cpython" }));
    await fireEvent.click(addButton());
    await fireEvent.click(button("Check"));
    const switchTo = await screen.findByRole("button", { name: "Switch to it" });
    const saving = Promise.withResolvers<unknown>();
    core.applySettings.mockReturnValue(saving.promise);
    // Apply sits under Add repository…, out of the user's reach, and a test can still press it.
    await fireEvent.click(button("Apply"));

    await fireEvent.click(switchTo);

    expect(addDialog()).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
    expect(onswitch).not.toHaveBeenCalled();
    expect(addButton().hasAttribute("disabled")).toBe(true);
    saving.resolve(ok(loaded(settingsWith((settings) => (settings.status_bar.visible = false)))));
    await vi.waitFor(() => {
      expect(onclose).toHaveBeenCalledOnce();
    });
    expect(onswitch).not.toHaveBeenCalled();
  });

  it("measures the Repositories only once the dialog has read the files", async () => {
    const reading = Promise.withResolvers<unknown>();
    core.rereadSettings.mockReturnValue(reading.promise);
    render(Settings, props());
    await go("Repositories");
    await nextTask();
    expect(core.onThisMachine).not.toHaveBeenCalled();
    expect(addButton().hasAttribute("disabled")).toBe(true);

    reading.resolve(ok(loaded(settingsWith())));
    await vi.waitFor(() => {
      expect(core.onThisMachine).toHaveBeenCalledOnce();
    });
    expect(addButton().hasAttribute("disabled")).toBe(false);
  });

  it("keeps Add repository… off when the dialog can't read the files", async () => {
    core.rereadSettings.mockResolvedValue(refused("settings.toml can't be read"));
    render(Settings, props());
    await go("Repositories");
    await screen.findByRole("alert");
    expect(addButton().hasAttribute("disabled")).toBe(true);
  });
});
