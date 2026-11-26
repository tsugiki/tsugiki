import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";
import type { About } from "$lib/state/about.svelte";
import { actionOn, remapKeys } from "$lib/state/keymap.svelte";
import { settingsAsk } from "$lib/state/settings-ask.svelte";
import {
  diffFile,
  done,
  inboxPage,
  listedIssue,
  loadedSettings,
  OCTOCAT_ONLINE,
  ok,
  openOn,
  readyFiles,
  refused,
  statusOf,
  workbenchPage,
  workbenchRow,
} from "$lib/testing/fixtures";
import { nextTask } from "$lib/testing/helpers";
import Page from "./+page.svelte";

const core = vi.hoisted(() => ({
  about: vi.fn(),
  openDevtools: vi.fn(),
  settings: vi.fn(),
  home: vi.fn(),
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  openRepository: vi.fn(),
  workbench: vi.fn(),
  inbox: vi.fn(),
  searchInbox: vi.fn(),
  saveFilter: vi.fn(),
  mirrorProblem: vi.fn(),
  checkMirrorsAgain: vi.fn(),
  mirrorQuestion: vi.fn(),
  mirrorStates: vi.fn(),
  chooseMirrorSource: vi.fn(),
  mirrorCost: vi.fn(),
  saveTheme: vi.fn(),
  themes: vi.fn(),
  turnWindow: vi.fn(),
  tokenSource: vi.fn(),
  rereadSettings: vi.fn(),
  applySettings: vi.fn(),
  recentCommands: vi.fn(),
  rememberCommand: vi.fn(),
  quickOpen: vi.fn(),
  fetchItem: vi.fn(),
  openOnGithub: vi.fn(),
  // The HUD logs a slow opening of an Issue whether or not it shows.
  recordMiss: vi.fn(() => Promise.resolve("a miss")),
  issue: vi.fn(),
  openIssue: vi.fn(),
  pullRequest: vi.fn(),
  openPullRequest: vi.fn(),
  pullRequestFiles: vi.fn(),
  pullRequestRows: vi.fn(),
  pullRequestSpans: vi.fn(),
  mirrorsHeld: vi.fn(),
  repositoryState: vi.fn(),
}));

/** What `settings.toml` says unless a test says otherwise: the defaults. */
const SETTINGS = loadedSettings();

/** The themes the core sends unless a test says otherwise. */
const THEMES = {
  theme: { kind: "system" },
  light: {
    chosen: "Light",
    all: [{ name: "Light", kind: "light", tokens: [{ property: "--bg", value: "#ffffff" }] }],
    failed: [],
  },
  dark: {
    chosen: "Dark",
    all: [
      { name: "Dark", kind: "dark", tokens: [{ property: "--bg", value: "#1f1f1f" }] },
      { name: "Gruvbox Dark Hard", kind: "dark", tokens: [{ property: "--bg", value: "#1d2021" }] },
    ],
    failed: [],
  },
  problems: [] as string[],
};

/** The listener for the mirrors' states, so a test can send it what the core would. */
const mirrorStates = vi.hoisted(() => ({ changed: vi.fn<(payload: unknown) => void>() }));
/** The listeners for the status, so a test can send them what the core would. */
const statuses = vi.hoisted(() => {
  const listeners = new Set<(event: { payload: unknown }) => void>();
  const changed = (payload: unknown) => {
    for (const listener of listeners) listener({ payload });
  };
  return { listeners, changed };
});
/** The listener for the time GitHub's points come back, so a test can say the rate limit's gate holds. */
const pointsBack = vi.hoisted(() => ({ changed: vi.fn<(payload: unknown) => void>() }));

vi.mock("$lib/data/bindings", async () => {
  const { itemMoves, prFilesEvents } = await import("$lib/testing/fixtures");
  return {
    commands: {
      ...itemMoves(),
      windowLoaded: async () => null,
      windowHidden: async () => null,
      windowShown: async () => null,
      status: async () => OCTOCAT_ONLINE,
      lastSync: async () => ({ repository: "some-maintainer/gcmon", at: null }),
      syncActivity: async () => null,
      hourPoints: async () => null,
      pointsBack: async () => null,
      inFlight: async () => null,
      cloning: async () => null,
      newVersion: async () => null,
      refresh: async () => null,
      languageServers: async () => [],
      repositorySync: async () => ({ status: "ok", data: { last_sync: null, open: null } }),
      ...core,
    },
    events: {
      ...prFilesEvents(() => async () => () => {}),
      acted: { listen: async () => () => {} },
      statusChanged: {
        // The status bar listens, and so does the open Repository's state.
        listen: async (callback: (event: { payload: unknown }) => void) => {
          statuses.listeners.add(callback);
          return () => {
            statuses.listeners.delete(callback);
          };
        },
      },
      lastSyncChanged: { listen: async () => () => {} },
      syncActivityChanged: { listen: async () => () => {} },
      hourPointsChanged: { listen: async () => () => {} },
      pointsBackChanged: {
        listen: async (callback: (event: { payload: unknown }) => void) => {
          pointsBack.changed.mockImplementation((payload) => {
            callback({ payload });
          });
          return () => {};
        },
      },
      inFlightChanged: { listen: async () => () => {} },
      mirrorProblemChanged: { listen: async () => () => {} },
      mirrorQuestionChanged: { listen: async () => () => {} },
      newVersionFound: { listen: async () => () => {} },
      mirrorStatesChanged: {
        listen: async (callback: (event: { payload: unknown }) => void) => {
          mirrorStates.changed.mockImplementation((payload) => {
            callback({ payload });
          });
          return () => {};
        },
      },
      languageServersChanged: { listen: async () => () => {} },
      // A Repository's page in Settings listens for news of its sync.
      repositorySyncChanged: { listen: async () => () => {} },
    },
  };
});

// jsdom has no `matchMedia`: the system stays on its dark side.
vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: () => {} }));

const onboarding = { screen: "onboarding", token: "gh" } as const;
const gcmon = { name: "some-maintainer/gcmon", conventions: "github" as const, filters: [] };
/** The window open on gcmon, the only Repository. */
const gcmonHome = openOn(gcmon);
/** The Workbench with one Issue, #7, on it. */
const oneIssue = workbenchPage([listedIssue(7, { title: "It crashes" })]);

/** A palette row's text without the key on its right, which describes the row and stays out of its name. */
const rowText = (option: HTMLElement) => {
  const row = option.cloneNode(true) as HTMLElement;
  row.querySelector("kbd")?.remove();
  return row.textContent;
};
/** The text of each of the palette's rows, without their keys. */
const rowTexts = () => screen.getAllByRole("option").map(rowText);
/** Presses Ctrl+Shift+P, which opens the command palette. */
const ctrlShiftP = () => fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
/** Presses Ctrl+K, which starts a chord. */
const ctrlK = () => fireEvent.keyDown(document.body, { key: "k", code: "KeyK", ctrlKey: true });

beforeEach(() => {
  // The tests share one document, so each starts with no title of its own.
  document.title = "";
  statuses.listeners.clear();
  core.repositoryState.mockResolvedValue({ status: "ok", data: { read_only: [], connection: "online" } });
  core.about.mockResolvedValue({ name: "Tsugiki", version: "1.2.3", build: "release", source_ref: "v1.2.3" });
  core.openDevtools.mockResolvedValue({ status: "ok", data: null });
  core.home.mockResolvedValue(onboarding);
  core.mirrorProblem.mockResolvedValue(null);
  core.mirrorQuestion.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.chooseMirrorSource.mockResolvedValue({ status: "ok", data: null });
  core.saveTheme.mockResolvedValue({ status: "ok", data: null });
  core.themes.mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
  core.turnWindow.mockResolvedValue(null);
  core.tokenSource.mockResolvedValue("gh");
  core.workbench.mockResolvedValue(workbenchPage([]));
  core.settings.mockResolvedValue(structuredClone(SETTINGS));
  core.recentCommands.mockResolvedValue([]);
  core.quickOpen.mockResolvedValue({ status: "ok", data: { named: null, others: [] } });
  // The core answers with the list it keeps, the command run first.
  let kept: string[] = [];
  core.rememberCommand.mockImplementation(async (id: string) => {
    kept = [id, ...kept.filter((one) => one !== id)];
    return { status: "ok", data: kept };
  });
  core.rereadSettings.mockResolvedValue({ status: "ok", data: structuredClone(SETTINGS) });
  core.applySettings.mockImplementation(async (changes: object) => ({
    status: "ok",
    data: {
      settings: { ...structuredClone(SETTINGS.settings), ...changes },
      language_servers: structuredClone(SETTINGS.language_servers),
      problems: [],
    },
  }));
});

afterEach(() => {
  vi.restoreAllMocks();
});

it("says when the Repository last synced, and a click asks again", async () => {
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Not synced yet" }));
  expect(await screen.findByRole("button", { name: "Not synced yet" })).toBeTruthy();
});

it("colours the window from the theme of the side the system is on", async () => {
  render(Page);
  await vi.waitFor(() => {
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe("#1f1f1f");
  });
  expect(document.documentElement.style.colorScheme).toBe("dark");
});

it("says why no mirror can be made, and a click asks the core to look again", async () => {
  core.mirrorProblem.mockResolvedValue({ kind: "no_git" });
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Check again" }));
  expect(screen.getByRole("status").textContent).toContain("can't find git");
  expect(core.checkMirrorsAgain).toHaveBeenCalledOnce();
});

describe("the dialog the status bar's question opens", () => {
  const dialog = () => screen.queryByRole("dialog", { name: "Mirror of python/cpython" });

  beforeEach(() => {
    core.mirrorQuestion.mockResolvedValue("python/cpython");
    core.mirrorCost.mockResolvedValue({
      status: "ok",
      data: { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 },
    });
  });

  const opened = async () => {
    render(Page);
    const line = await screen.findByRole("button", { name: "python/cpython's mirror waits for your answer" });
    // jsdom doesn't focus a button it clicks, as a browser does.
    line.focus();
    await fireEvent.click(line);
    return line;
  };

  it("asks where the mirror gets its objects, hands the core the answer and closes", async () => {
    await opened();
    expect(dialog()).toBeTruthy();
    expect(core.mirrorCost).toHaveBeenCalledWith("python/cpython");
    expect(
      await screen.findByRole("radio", { name: "Download it from GitHub, about 3 minutes" }),
    ).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Start the clone" }));

    expect(core.chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "download" });
    await vi.waitFor(() => {
      expect(dialog()).toBeNull();
    });
  });

  it("takes the focus, and closes on Esc, Close or a click outside, with the focus back", async () => {
    const line = await opened();
    expect(document.activeElement).toBe(dialog());
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(line);

    await fireEvent.click(line);
    await fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(dialog()).toBeNull();

    await fireEvent.click(line);
    await fireEvent.mouseDown(dialog() as HTMLElement);
    expect(dialog()).toBeTruthy();
    // A browser would move the focus to the body after the dialog gave it back, if not stopped.
    expect(await fireEvent.mouseDown(screen.getByTestId("mirror-question-scrim"))).toBe(false);
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(line);
    expect(core.chooseMirrorSource).not.toHaveBeenCalled();
  });

  it("keeps the palette from opening on top", async () => {
    await opened();
    await fireEvent.keyDown(window, { key: "p", code: "KeyP", ctrlKey: true, shiftKey: true });
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
  });
});

it("opens on onboarding with no Repositories", async () => {
  render(Page);
  expect(await screen.findByText(/Your token comes from/)).toBeTruthy();
  expect(document.title).toBe("Tsugiki 1.2.3");
});

it("reopens the Repository the core names, and the window title names it", async () => {
  core.home.mockResolvedValue(gcmonHome);
  render(Page);
  expect(await screen.findByRole("button", { name: gcmon.name })).toBeTruthy();
  expect(document.title).toBe("some-maintainer/gcmon · Tsugiki 1.2.3");
});

it("saves the Inbox search as a Filter from its Save dialog, then opens it with the box cleared", async () => {
  core.home.mockResolvedValue(gcmonHome);
  core.inbox.mockResolvedValue(inboxPage([]));
  const bugs = { name: "Bugs", query: "label:bug is:open", on_workbench: false, count: 0, problem: null };
  core.workbench.mockResolvedValue(workbenchPage([], { filters: [bugs] }));
  core.saveFilter.mockResolvedValue(done);
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));
  await screen.findByRole("heading", { name: "Inbox" });

  core.inbox.mockResolvedValue(
    inboxPage([listedIssue(7, { title: "It crashes" })], null, "off", false, null, "label:bug is:open"),
  );
  await fireEvent.input(screen.getByRole("searchbox", { name: "Search the Inbox" }), {
    target: { value: "label:bug" },
  });
  // The row lands once the core's answer for this text is on show, which spells out the query.
  await screen.findByRole("button", { name: "#7 It crashes" });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));

  const dialog = await screen.findByRole("dialog", { name: "Save as Filter" });
  const name = within(dialog).getByRole("textbox", { name: "Name" }) as HTMLInputElement;
  const query = within(dialog).getByRole("textbox", { name: "Query" }) as HTMLInputElement;
  expect(name.value).toBe("label:bug");
  expect(query.value).toBe("label:bug is:open");
  await fireEvent.input(name, { target: { value: "Bugs" } });
  await fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

  await vi.waitFor(() => {
    expect(core.saveFilter).toHaveBeenCalledExactlyOnceWith(
      "some-maintainer/gcmon",
      "Bugs",
      "label:bug is:open",
    );
  });
  await vi.waitFor(() => {
    expect(screen.queryByRole("dialog", { name: "Save as Filter" })).toBeNull();
  });
  expect(await screen.findByRole("heading", { name: "Bugs" })).toBeTruthy();
  expect(screen.queryByRole("searchbox", { name: "Search the Inbox" })).toBeNull();
  expect(core.workbench).toHaveBeenLastCalledWith("some-maintainer/gcmon", {
    kind: "filter",
    query: "label:bug is:open",
  });
});

it("keeps the Save dialog open and says why the core refused the Filter's name", async () => {
  core.home.mockResolvedValue(gcmonHome);
  core.inbox.mockResolvedValue(inboxPage([]));
  const refusal = "some-maintainer/gcmon already has a Filter named Bugs. Choose another name.";
  core.saveFilter.mockResolvedValue(refused(refusal));
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));
  await screen.findByRole("heading", { name: "Inbox" });
  core.inbox.mockResolvedValue(
    inboxPage([listedIssue(7, { title: "It crashes" })], null, "off", false, null, "label:bug is:open"),
  );
  await fireEvent.input(screen.getByRole("searchbox", { name: "Search the Inbox" }), {
    target: { value: "label:bug" },
  });
  await screen.findByRole("button", { name: "#7 It crashes" });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  const dialog = await screen.findByRole("dialog", { name: "Save as Filter" });

  await fireEvent.input(within(dialog).getByRole("textbox", { name: "Name" }), {
    target: { value: "Bugs" },
  });
  await fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

  expect((await within(dialog).findByRole("alert")).textContent).toBe(refusal);
  expect(screen.getByRole("dialog", { name: "Save as Filter" })).toBeTruthy();
  // The Inbox stays behind the dialog, with its search still in the box.
  expect((screen.getByRole("searchbox", { name: "Search the Inbox" }) as HTMLInputElement).value).toBe(
    "label:bug",
  );
  // An edit says the refusal spoke of what the fields held, so it goes.
  await fireEvent.input(within(dialog).getByRole("textbox", { name: "Name" }), {
    target: { value: "Crash bugs" },
  });
  expect(within(dialog).queryByRole("alert")).toBeNull();
  await fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await within(dialog).findByRole("alert");
  await fireEvent.input(within(dialog).getByRole("textbox", { name: "Query" }), {
    target: { value: "label:crash is:open" },
  });
  expect(within(dialog).queryByRole("alert")).toBeNull();
});

it("says a Read-only Repository is so in the status bar and atop its Issue view, until the core says otherwise", async () => {
  const state = (read_only: "token"[]) => ({ status: "ok", data: { read_only, connection: "online" } });
  core.home.mockResolvedValue(gcmonHome);
  core.workbench.mockResolvedValue(oneIssue);
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.issue.mockResolvedValue({ status: "ok", data: null });
  core.repositoryState.mockResolvedValue(state(["token"]));
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "#7 It crashes" }));

  const words = "Read-only: your token can't write to some-maintainer/gcmon";
  expect(await screen.findByText(words)).toBeTruthy();
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent?.trim()).toBe("octocat · Read-only");
  });
  expect(core.repositoryState).toHaveBeenCalledWith("some-maintainer/gcmon");

  // The status can say Tsugiki went offline or came back, so the state is asked for again.
  core.repositoryState.mockResolvedValue(state([]));
  statuses.changed(OCTOCAT_ONLINE);
  await vi.waitFor(() => {
    expect(screen.queryByText(words)).toBeNull();
  });
  expect(screen.getByRole("status").textContent?.trim()).toBe("octocat");
});

it("says a Repository Read-only by choice is so in the status bar, and its line opens its page in Settings", async () => {
  core.home.mockResolvedValue(gcmonHome);
  core.workbench.mockResolvedValue(oneIssue);
  core.openIssue.mockResolvedValue({ status: "ok", data: null });
  core.issue.mockResolvedValue({ status: "ok", data: null });
  core.repositoryState.mockResolvedValue({
    status: "ok",
    data: { read_only: ["chosen"], connection: "online" },
  });
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "#7 It crashes" }));

  const link = await screen.findByRole("button", { name: "this Repository's settings" });
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent?.trim()).toBe("octocat · Read-only");
  });
  await fireEvent.click(link);

  const settings = await screen.findByRole("dialog", { name: "Settings" });
  expect(within(settings).getByRole("region", { name: "some-maintainer/gcmon" })).toBeTruthy();
});

it("says so when the core can't say what to open", async () => {
  core.home.mockRejectedValue(new Error("the core has gone"));
  render(Page);
  expect(await screen.findByText(/can't say what to open: Error: the core has gone/)).toBeTruthy();
  expect(document.title).toBe("Tsugiki 1.2.3");
});

it("adds a Repository from onboarding and then shows it", async () => {
  core.addRepository.mockResolvedValue({ status: "ok", data: gcmonHome });
  core.checkRepository.mockResolvedValue({
    status: "ok",
    data: {
      kind: "new",
      check: {
        repository: gcmon,
        open_issues: 10,
        open_prs: 3,
        size: "small",
        window_days: 30,
        first_sync_items: 48,
        first_sync_seconds: 5,
        first_sync_points: 12,
        mirror: { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 },
      },
    },
  });
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  expect(document.title).toBe("Tsugiki 1.2.3");
  await fireEvent.click(screen.getByRole("button", { name: "Check" }));
  await fireEvent.click(await screen.findByRole("button", { name: "Add" }));

  await screen.findByText(/Nothing here yet/);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("button", { name: gcmon.name })).toBeTruthy();
  expect(document.title).toBe("some-maintainer/gcmon · Tsugiki 1.2.3");
  expect(core.settings).toHaveBeenCalledTimes(2);
});

it("keeps the palette from opening over the add dialog", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  await ctrlShiftP();
  expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
  expect(screen.getByRole("dialog", { name: "Add repository" })).toBeTruthy();
});

it("closes the add dialog without changing anything", async () => {
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByText(/Your token comes from/)).toBeTruthy();
});

it("gives the focus back from the add dialog on Esc, on Cancel and on a press outside", async () => {
  render(Page);
  const add = await screen.findByRole("button", { name: "Add repository" });
  const dialog = () => screen.queryByRole("dialog", { name: "Add repository" });
  /** Opens the dialog from the button, which a browser focuses on the click and jsdom doesn't. */
  const open = async () => {
    add.focus();
    await fireEvent.click(add);
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Repository" }));
  };

  await open();
  await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(add);

  await open();
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(add);

  await open();
  expect(await fireEvent.mouseDown(screen.getByTestId("add-repository-scrim"))).toBe(false);
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(add);
});

it("names the build in the window's title once the core answers, and nowhere on the screen", async () => {
  let answer: (about: About) => void = () => {};
  core.about.mockReturnValue(new Promise<About>((resolve) => (answer = resolve)));
  core.home.mockResolvedValue(gcmonHome);
  render(Page);
  await screen.findByRole("button", { name: gcmon.name });
  expect(document.title).toBe("some-maintainer/gcmon · Tsugiki");

  answer({ name: "Tsugiki", version: "0.3.2 (6143447)", build: "release", source_ref: "v0.3.2" });
  await vi.waitFor(() => {
    expect(document.title).toBe("some-maintainer/gcmon · Tsugiki 0.3.2 (6143447)");
  });
  expect(screen.queryByText(/0\.3\.2/)).toBeNull();
});

it("says in the status bar when the core doesn't answer, and names the app alone in the title", async () => {
  core.about.mockRejectedValue(new Error("no IPC"));
  render(Page);
  await vi.waitFor(() => {
    expect(screen.getByRole("status").textContent).toContain("The core didn't answer: Error: no IPC");
  });
  expect(document.title).toBe("Tsugiki");
});

it("shows who is signed in", async () => {
  render(Page);
  expect(await screen.findByText("octocat")).toBeTruthy();
});

it("opens the command palette on Ctrl+Shift+P and closes it on Esc", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  render(Page);
  expect(screen.queryByRole("dialog")).toBeNull();
  await ctrlShiftP();
  expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
  await fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("gives the list's keys to a dialog on top of it", async () => {
  core.home.mockResolvedValue(gcmonHome);
  core.workbench.mockResolvedValue(oneIssue);
  render(Page);
  const row = await screen.findByRole("button", { name: "#7 It crashes" });

  await ctrlShiftP();
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).not.toBe(row);

  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).toBe(row);
});

describe("the focus under the palette", () => {
  const palette = () => screen.queryByRole("dialog", { name: "Command palette" });

  /** The page open on gcmon, with the focus on its one row. */
  const onRow = async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
    core.workbench.mockResolvedValue(oneIssue);
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });
    expect(document.activeElement).toBe(row);
    return row;
  };

  /** Opens the palette and runs `title`. The palette hands the focus back before the command moves it. */
  const fromPalette = async (title: string) => {
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: title }));
  };
  const detailPane = () => document.querySelector('[data-pane="detail"]');

  it("goes back to the row on Esc", async () => {
    const row = await onRow();
    await ctrlShiftP();
    expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Command" }));
    await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(palette()).toBeNull();
    expect(document.activeElement).toBe(row);
  });

  it("goes back to the row on a press outside, which the browser doesn't then move", async () => {
    const row = await onRow();
    await ctrlShiftP();
    expect(await fireEvent.mouseDown(screen.getByTestId("palette-scrim"))).toBe(false);
    expect(palette()).toBeNull();
    expect(document.activeElement).toBe(row);
  });

  it("goes where the command the user runs puts it", async () => {
    await onRow();
    await fromPalette("Show Only the Detail Pane");
    expect(palette()).toBeNull();
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(detailPane());
    });
  });

  it("goes to the Detail pane on Hide Issue List and back to the row on Show Issue List, as on Ctrl+Alt+B", async () => {
    await onRow();

    await fromPalette("Hide Issue List");
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(detailPane());
    });
    await fromPalette("Show Issue List");

    // The list draws its rows afresh as it comes back.
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "#7 It crashes" }));
    });
  });

  it("goes to the row on Hide Sidebar from a Sidebar button, and stays there on Show Sidebar, as on Ctrl+B", async () => {
    const row = await onRow();
    const sidebar = screen.getByRole("navigation", { name: "Sidebar" });
    within(sidebar).getByRole("button", { name: gcmon.name }).focus();

    await fromPalette("Hide Sidebar");
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(row);
    });
    await fromPalette("Show Sidebar");
    // The command moves the focus, if it does, on the tick after the Sidebar changes.
    await screen.findByRole("navigation", { name: "Sidebar" });
    await tick();

    expect(document.activeElement).toBe(row);
  });

  // A command that ran before the palette gave the focus back would find the focus gone with the palette, and
  // move it to the Detail pane itself.
  it("stays on a control in the Detail pane on Hide Sidebar, as on Ctrl+B", async () => {
    await onRow();
    await fireEvent.keyDown(document.activeElement as Element, {
      key: "b",
      code: "KeyB",
      ctrlKey: true,
      altKey: true,
    });
    const control = await within(screen.getByRole("region", { name: "Detail pane" })).findByRole("button");
    control.focus();

    await fromPalette("Hide Sidebar");
    await vi.waitFor(() => {
      expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
    });
    await tick();

    expect(document.activeElement).toBe(control);
  });
});

describe("the performance HUD", () => {
  const ctrlShiftH = () =>
    fireEvent.keyDown(window, { key: "H", code: "KeyH", ctrlKey: true, shiftKey: true });
  const panel = () => screen.queryByRole("complementary", { name: "Performance HUD" });

  it("shows and hides on Ctrl+Shift+H", async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    render(Page);
    expect(panel()).toBeNull();
    await ctrlShiftH();
    expect(panel()).toBeTruthy();
    await ctrlShiftH();
    expect(panel()).toBeNull();
  });

  it("shows and hides from the command palette", async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    render(Page);
    const fromPalette = async (title: string) => {
      await ctrlShiftP();
      await fireEvent.click(screen.getByRole("option", { name: title }));
    };
    await fromPalette("Show performance HUD");
    expect(panel()).toBeTruthy();
    await fromPalette("Hide performance HUD");
    expect(panel()).toBeNull();
  });
});

describe("Open DevTools", () => {
  /** Opens the palette and gives the title of each of its rows. */
  const paletteTitles = async () => {
    await ctrlShiftP();
    return rowTexts();
  };

  it("is in a development build's palette, and asks the core to open DevTools", async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.about.mockResolvedValue({
      name: "Tsugiki",
      version: "1.2.3",
      build: "development",
      source_ref: "main",
    });
    render(Page);
    await vi.waitFor(() => {
      expect(document.title).toBe("Tsugiki 1.2.3");
    });
    expect(await paletteTitles()).toContain("Open DevTools");
    await fireEvent.click(screen.getByRole("option", { name: "Open DevTools" }));
    expect(core.openDevtools).toHaveBeenCalledOnce();
  });

  it("isn't in a release's palette", async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    render(Page);
    await vi.waitFor(() => {
      expect(document.title).toBe("Tsugiki 1.2.3");
    });
    expect(await paletteTitles()).not.toContain("Open DevTools");
  });
});

const peps = { name: "python/peps", conventions: "github" as const, filters: [] };
/** The window open on gcmon, with peps opened before it. */
const gcmonAfterPeps = openOn(gcmon, [peps.name]);
/** The window switched to peps. */
const pepsHome = openOn(peps, [gcmon.name]);
/** Each option's text, with the space before a row's "open" mark as a plain one. */
const spaced = (option: HTMLElement) => option.textContent.replace(/\s/g, " ");
const options = () => screen.getAllByRole("option").map(spaced);
/** Opens Add repository from gcmon's window: the Repository's name atop the Sidebar opens the picker, whose
 * last row is Add repository…. */
const openAddFromPicker = async () => {
  await fireEvent.click(await screen.findByRole("button", { name: gcmon.name }));
  await fireEvent.click(screen.getByRole("option", { name: "Add repository…" }));
};

/** A Standalone PR, #70. */
const pr = workbenchRow(70, { title: "gh-7: Fix the crash", author: "some-author", ci: null });
/** #70's diff, of three files. */
const diff = ok(
  readyFiles(3, {
    files: ["Lib/gc.py", "Modules/gcmodule.c", "Lib/test/test_70.py"].map((path, row) => diffFile(path, row)),
  }),
);

/** The core with #70 alone on the Workbench, its view and its diff. */
function listOnePr() {
  core.workbench.mockResolvedValue(
    workbenchPage([{ kind: "standalone_pr", ...pr, backports: [], attention: null }]),
  );
  core.openPullRequest.mockResolvedValue(done);
  core.pullRequest.mockResolvedValue({
    status: "ok",
    data: { ...pr, content: null, shares_review_of: null, hidden: null, issues: [] },
  });
  core.mirrorsHeld.mockResolvedValue(false);
  core.pullRequestFiles.mockResolvedValue(diff);
  core.pullRequestRows.mockResolvedValue({ status: "ok", data: [] });
  core.pullRequestSpans.mockResolvedValue({ status: "ok", data: [] });
}

describe("quick open", () => {
  const ctrlP = () => fireEvent.keyDown(window, { key: "p", code: "KeyP", ctrlKey: true });
  const quickOpen = () => screen.queryByRole("dialog", { name: "Quick open" });
  const box = () => screen.getByRole("combobox");
  const found = [
    { kind: "issue", number: 7, title: "It crashes", state: "OPEN" },
    { kind: "pull_request", number: 70, title: "gh-7: Fix the crash", state: "OPEN", is_draft: true },
    { kind: "issue", number: 700, title: "It crashed long ago", state: null },
  ];

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
    core.workbench.mockResolvedValue(oneIssue);
    core.quickOpen.mockResolvedValue({ status: "ok", data: { named: null, others: found } });
    core.openIssue.mockResolvedValue({ status: "ok", data: null });
    core.issue.mockResolvedValue({ status: "ok", data: null });
  });

  it("opens on Ctrl+P with nothing typed, and lists what the core finds in the open Repository", async () => {
    render(Page);
    await screen.findByRole("button", { name: "#7 It crashes" });

    await ctrlP();
    await screen.findByRole("option", { name: /^#7 / });

    expect(core.quickOpen).toHaveBeenCalledWith("some-maintainer/gcmon", "");
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "#7 It crashes · Open issue",
      "#70 gh-7: Fix the crash · Draft PR",
      "#700 It crashed long ago · Issue",
    ]);
    expect((box() as HTMLInputElement).value).toBe("");
  });

  it("opens the item typed on Enter, and keeps it out of the palette's history", async () => {
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await ctrlP();
    await fireEvent.input(box(), { target: { value: "#7" } });
    await vi.waitFor(() => {
      expect(core.quickOpen).toHaveBeenLastCalledWith("some-maintainer/gcmon", "#7");
    });

    await fireEvent.keyDown(box(), { key: "Enter" });

    await vi.waitFor(() => {
      expect(core.openIssue).toHaveBeenCalledWith("some-maintainer/gcmon", 7);
    });
    expect(quickOpen()).toBeNull();
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(row);
    });
    expect(core.rememberCommand).not.toHaveBeenCalled();
  });

  describe("for a number the database holds no item of", () => {
    /** The answer to the fetch of #12, which comes once the test calls the function this gives. */
    let answerFetch: (answer: object) => void;

    beforeEach(() => {
      core.quickOpen.mockResolvedValue({ status: "ok", data: { named: null, others: [], unstored: 12 } });
      core.fetchItem.mockReturnValue(
        new Promise((resolve) => {
          answerFetch = (answer) => {
            resolve({ status: "ok", data: answer });
          };
        }),
      );
      core.openOnGithub.mockResolvedValue(done);
    });

    const detailPane = () => screen.getByRole("region", { name: "Detail pane" });

    /** The window with Enter pressed on quick open's row for #12. */
    const fetchTwelve = async () => {
      render(Page);
      await screen.findByRole("button", { name: "#7 It crashes" });
      await ctrlP();
      await fireEvent.input(box(), { target: { value: "#12" } });
      const row = await screen.findByRole("option", { name: "Fetch #12 from GitHub" });
      const rows = rowTexts();
      await fireEvent.keyDown(box(), { key: "Enter" });
      return { row, rows };
    };

    it("lists the row that fetches it, and shows it loading in the Detail pane", async () => {
      const { rows } = await fetchTwelve();

      expect(rows).toEqual(["Fetch #12 from GitHub"]);
      expect(quickOpen()).toBeNull();
      expect(core.fetchItem).toHaveBeenCalledWith("some-maintainer/gcmon", 12);
      const pane = within(detailPane());
      expect(pane.getByRole("heading", { name: "some-maintainer/gcmon #12" })).toBeTruthy();
      expect(pane.getByRole("status").textContent).toBe("Fetching some-maintainer/gcmon #12 from GitHub…");
    });

    /** Quick open's rows for #12 once `hold` has told the window GitHub can't be asked, and the browser
     * opened by Enter on the first. */
    const browsingTwelve = async (hold: () => void) => {
      render(Page);
      await screen.findByRole("button", { name: "#7 It crashes" });
      hold();
      await ctrlP();
      await fireEvent.input(box(), { target: { value: "#12" } });
      await screen.findByRole("option", { name: "Open #12 in browser" });
      const rows = rowTexts();

      await fireEvent.keyDown(box(), { key: "Enter" });

      expect(rows).toEqual(["Open #12 in browser"]);
      await vi.waitFor(() => {
        expect(core.openOnGithub).toHaveBeenCalledWith("some-maintainer/gcmon", {
          kind: "issue",
          number: 12,
        });
      });
      expect(core.fetchItem).not.toHaveBeenCalled();
    };

    it("offers it in the browser in place of a fetch while GitHub can't be reached", async () => {
      await browsingTwelve(() => {
        statuses.changed(statusOf({ state: "offline_as", last_login: "octocat" }));
      });
    });

    it("offers it in the browser in place of a fetch while the rate limit holds every request", async () => {
      await browsingTwelve(() => {
        pointsBack.changed({ at: "2026-11-21T14:00:00Z" });
      });
    });

    it("offers the fetch again once GitHub's points come back", async () => {
      render(Page);
      await screen.findByRole("button", { name: "#7 It crashes" });
      pointsBack.changed({ at: "2026-11-21T14:00:00Z" });
      pointsBack.changed(null);
      await ctrlP();
      await fireEvent.input(box(), { target: { value: "#12" } });

      expect(await screen.findByRole("option", { name: "Fetch #12 from GitHub" })).toBeTruthy();
    });

    it("opens an Issue the fetch found as an Issue", async () => {
      await fetchTwelve();

      answerFetch({ kind: "issue" });

      await vi.waitFor(() => {
        expect(core.openIssue).toHaveBeenCalledWith("some-maintainer/gcmon", 12);
      });
      expect(screen.queryByRole("heading", { name: "some-maintainer/gcmon #12" })).toBeNull();
    });

    it("opens a PR the fetch found as a PR", async () => {
      core.openPullRequest.mockResolvedValue(done);
      core.pullRequest.mockResolvedValue({ status: "ok", data: null });
      core.mirrorsHeld.mockResolvedValue(false);
      await fetchTwelve();

      answerFetch({ kind: "pull_request" });

      await vi.waitFor(() => {
        expect(core.openPullRequest).toHaveBeenCalledWith("some-maintainer/gcmon", 12);
      });
    });

    it("says why the fetch brought nothing, and opens the number on GitHub", async () => {
      await fetchTwelve();

      answerFetch({ kind: "unfetched", why: "missing" });
      const why = await within(detailPane()).findByRole("alert");
      await fireEvent.click(within(detailPane()).getByRole("button", { name: "Open in browser" }));

      expect(why.textContent).toBe("GitHub has no Issue or PR #12 in some-maintainer/gcmon.");
      expect(core.openOnGithub).toHaveBeenCalledWith("some-maintainer/gcmon", { kind: "issue", number: 12 });
    });

    it("opens the number it fetches on Open on GitHub, and not the item under the cursor", async () => {
      await fetchTwelve();

      await fireEvent.keyDown(document.body, { key: "G", code: "KeyG", ctrlKey: true, shiftKey: true });

      await vi.waitFor(() => {
        expect(core.openOnGithub).toHaveBeenCalledExactlyOnceWith("some-maintainer/gcmon", {
          kind: "issue",
          number: 12,
        });
      });
    });

    it("takes the focus with the list hidden, and leaves it in a text box the user went to meanwhile", async () => {
      render(Page);
      await screen.findByRole("button", { name: "#7 It crashes" });
      await fireEvent.keyDown(document.body, { key: "b", code: "KeyB", ctrlKey: true, altKey: true });
      await vi.waitFor(() => {
        expect(screen.queryByRole("button", { name: "#7 It crashes" })).toBeNull();
      });
      await ctrlP();
      await fireEvent.input(box(), { target: { value: "#12" } });
      await screen.findByRole("option", { name: "Fetch #12 from GitHub" });
      await fireEvent.keyDown(box(), { key: "Enter" });
      await vi.waitFor(() => {
        expect(document.activeElement).toBe(detailPane());
      });
      const typing = document.body.appendChild(document.createElement("input"));
      onTestFinished(() => {
        typing.remove();
      });
      typing.focus();

      answerFetch({ kind: "issue" });
      await vi.waitFor(() => {
        expect(core.openIssue).toHaveBeenCalledWith("some-maintainer/gcmon", 12);
      });
      await nextTask();
      await tick();

      expect(document.activeElement).toBe(typing);
    });

    it("opens nothing it found once the user opened another item meanwhile", async () => {
      await fetchTwelve();
      await fireEvent.click(screen.getByRole("button", { name: "#7 It crashes" }));

      answerFetch({ kind: "issue" });
      await nextTask();

      expect(core.openIssue).toHaveBeenCalledWith("some-maintainer/gcmon", 7);
      expect(core.openIssue).not.toHaveBeenCalledWith("some-maintainer/gcmon", 12);
    });
  });

  it("switches to the commands on `>`, and Ctrl+Shift+P opens with `>` typed", async () => {
    render(Page);
    await screen.findByRole("button", { name: "#7 It crashes" });
    await ctrlP();
    await fireEvent.input(box(), { target: { value: ">settings" } });
    const commands = rowTexts();
    await fireEvent.keyDown(window, { key: "Escape" });

    await ctrlShiftP();

    expect(commands).toEqual(["Open Settings"]);
    expect((box() as HTMLInputElement).value).toBe(">");
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
  });

  describe("with a PR on show", () => {
    const textsOf = () => screen.getAllByRole("option").map((option) => option.textContent);

    beforeEach(listOnePr);

    /** The window with the PR on show on its Conversation tab, and quick open opened over it. */
    const openOverPr = async () => {
      render(Page);
      await fireEvent.click(await screen.findByRole("button", { name: "#70 gh-7: Fix the crash" }));
      await screen.findByRole("tab", { name: "Conversation", selected: true });
      await ctrlP();
    };

    it("lists the files of the PR the text holds first, and opens the one picked on its Files tab", async () => {
      await openOverPr();
      await screen.findByRole("option", { name: /^#7 / });
      const unread = [textsOf(), core.pullRequestFiles.mock.calls.length];

      await fireEvent.input(box(), { target: { value: "GC" } });
      await screen.findByRole("option", { name: /^Modules/ });
      const rows = textsOf();
      await fireEvent.keyDown(box(), { key: "ArrowDown" });
      await fireEvent.keyDown(box(), { key: "Enter" });

      // Nothing typed lists no files, and reads no diff.
      expect(unread).toEqual([
        [
          "#7 It crashes · Open issue",
          "#70 gh-7: Fix the crash · Draft PR",
          "#700 It crashed long ago · Issue",
        ],
        0,
      ]);
      expect(rows).toEqual([
        "Lib/gc.py · File",
        "Modules/gcmodule.c · File",
        "#7 It crashes · Open issue",
        "#70 gh-7: Fix the crash · Draft PR",
        "#700 It crashed long ago · Issue",
      ]);
      expect(await screen.findByRole("tab", { name: "Files", selected: true })).toBeTruthy();
      expect(quickOpen()).toBeNull();
    });

    it("puts the item the text names by its number before the files", async () => {
      const [seven, seventy, sevenHundred] = found;
      core.quickOpen.mockResolvedValue({
        status: "ok",
        data: { named: seventy, others: [seven, sevenHundred] },
      });
      await openOverPr();
      await fireEvent.input(box(), { target: { value: "70" } });
      await screen.findByRole("option", { name: /^Lib\/test/ });

      expect(textsOf()).toEqual([
        "#70 gh-7: Fix the crash · Draft PR",
        "Lib/test/test_70.py · File",
        "#7 It crashes · Open issue",
        "#700 It crashed long ago · Issue",
      ]);
    });

    it("lists the files when the core can't search, above why", async () => {
      core.quickOpen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
      await openOverPr();
      await fireEvent.input(box(), { target: { value: "gc" } });

      await screen.findByRole("option", { name: /^Modules/ });

      expect(textsOf()).toEqual(["Lib/gc.py · File", "Modules/gcmodule.c · File"]);
      expect(within(screen.getByRole("dialog")).getByRole("alert").textContent).toBe(
        "Tsugiki can't search the Issues and PRs: gone",
      );
    });

    it("holds an Enter pressed before the diff comes, and opens the file it finds", async () => {
      let answer: (diff: unknown) => void = () => {};
      core.pullRequestFiles.mockReturnValue(
        new Promise((resolve) => {
          answer = resolve;
        }),
      );
      await openOverPr();
      await fireEvent.input(box(), { target: { value: "gcmodule" } });
      await vi.waitFor(() => {
        expect(core.pullRequestFiles).toHaveBeenCalled();
      });
      await screen.findByRole("option", { name: /^#7 / });

      await fireEvent.keyDown(box(), { key: "Enter" });
      const early = quickOpen();
      answer(diff);

      expect(early).not.toBeNull();
      expect(await screen.findByRole("tab", { name: "Files", selected: true })).toBeTruthy();
      expect(quickOpen()).toBeNull();
      expect(core.openIssue).not.toHaveBeenCalled();
    });

    it("opens the item the text names by its number on Enter at once, with no wait for the diff", async () => {
      core.pullRequestFiles.mockReturnValue(new Promise(() => {}));
      const [seven, ...others] = found;
      core.quickOpen.mockResolvedValue({ status: "ok", data: { named: seven, others } });
      await openOverPr();
      await fireEvent.input(box(), { target: { value: "7" } });
      await vi.waitFor(() => {
        expect(core.pullRequestFiles).toHaveBeenCalled();
      });
      await screen.findByRole("option", { name: /^#7 / });

      await fireEvent.keyDown(box(), { key: "Enter" });

      await vi.waitFor(() => {
        expect(core.openIssue).toHaveBeenCalledWith("some-maintainer/gcmon", 7);
      });
      expect(quickOpen()).toBeNull();
    });
  });

  it("says why the core couldn't search", async () => {
    core.quickOpen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
    render(Page);
    await screen.findByRole("button", { name: "#7 It crashes" });

    await ctrlP();

    const dialog = await screen.findByRole("dialog", { name: "Quick open" });
    expect((await within(dialog).findByRole("alert")).textContent).toBe(
      "Tsugiki can't search the Issues and PRs: gone",
    );
  });

  it("does nothing on onboarding, which has no Repository to search, whose palette has no `>`", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByText(/Your token comes from/);

    await ctrlP();
    const opened = screen.queryByRole("dialog");
    await ctrlShiftP();

    expect(opened).toBeNull();
    expect((box() as HTMLInputElement).value).toBe("");
    expect(core.quickOpen).not.toHaveBeenCalled();
  });
});

describe("a switch from the palette", () => {
  const palette = async () => {
    await ctrlShiftP();
  };

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonAfterPeps);
  });

  it("offers each Repository but the open one", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await palette();
    expect(options()).toContain("Repository: python/peps");
    expect(options()).not.toContain("Repository: some-maintainer/gcmon");
  });

  it("shows the Repository it switches to as a launch does, with its own title and last sync", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    render(Page);
    await screen.findByRole("button", { name: "Not synced yet" });
    await palette();
    await fireEvent.click(screen.getByRole("option", { name: "Repository: python/peps" }));

    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(core.openRepository).toHaveBeenCalledWith("python/peps");
    expect(core.workbench).toHaveBeenLastCalledWith("python/peps", { kind: "whole" });
    expect(document.title).toBe("python/peps · Tsugiki 1.2.3");
    // A switch keeps out of the palette's history: these rows come most recently opened first.
    expect(core.rememberCommand).not.toHaveBeenCalled();
    // The core read the files again to switch, so a problem the user has since fixed can go.
    await vi.waitFor(() => {
      expect(core.settings).toHaveBeenCalledTimes(2);
    });
    // The last sync the core told of is the Repository left behind's, until the core tells of this one's.
    expect(screen.queryByRole("button", { name: "Not synced yet" })).toBeNull();
    await palette();
    expect(options()).toContain("Repository: some-maintainer/gcmon");
    expect(options()).not.toContain("Repository: python/peps");
  });

  it("stays on the open Repository and says why when the core refuses", async () => {
    core.openRepository.mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
    });
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await palette();
    await fireEvent.click(screen.getByRole("option", { name: "Repository: python/peps" }));

    expect(
      await screen.findByText(
        "Tsugiki couldn't switch to python/peps: machine.toml, line 1: expected a value",
      ),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: gcmon.name })).toBeTruthy();
    expect(document.title).toBe("some-maintainer/gcmon · Tsugiki 1.2.3");
  });
});

describe("the palette's order", () => {
  const palette = async () => {
    await ctrlShiftP();
  };
  const titles = rowTexts;

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonAfterPeps);
  });

  it("puts the command run last at the top, and typing keeps the matches in that order", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await palette();
    const before = titles();
    await fireEvent.click(screen.getByRole("option", { name: "Reload Themes" }));
    await palette();
    await fireEvent.click(screen.getByRole("option", { name: "Reset Layout" }));

    await palette();
    const after = titles();
    await fireEvent.input(screen.getByRole("combobox", { name: "Command" }), { target: { value: ">re" } });

    expect(before.slice(0, 2)).toEqual(["Hide Sidebar", "Hide Issue List"]);
    expect(after.slice(0, 3)).toEqual(["Reset Layout", "Reload Themes", "Hide Sidebar"]);
    expect(titles()).toEqual([
      "Reset Layout",
      "Reload Themes",
      "Previous Pane",
      "Switch Repository",
      "View: Stale Reviews",
      "Repository: python/peps",
    ]);
    expect(core.rememberCommand.mock.calls).toEqual([["Reload Themes"], ["Reset Layout"]]);
  });

  it("opens with the commands the core kept first, as after a restart, each known by its id", async () => {
    core.recentCommands.mockResolvedValue(["Open Settings", "toggle_hud"]);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });

    await palette();

    expect(titles().slice(0, 3)).toEqual(["Open Settings", "Show performance HUD", "Hide Sidebar"]);
  });

  it("says in the status bar why a command run can't be saved", async () => {
    core.rememberCommand.mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "machine.toml:1: expected `]`" },
    });
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await palette();

    await fireEvent.click(screen.getByRole("option", { name: "Reset Layout" }));

    expect(
      await screen.findByText(
        "Tsugiki can't save the palette's recent commands: machine.toml:1: expected `]`",
        { exact: false },
      ),
    ).toBeTruthy();
  });

  it("takes a hand edit to the list once the Settings dialog has read the files again", async () => {
    core.recentCommands.mockResolvedValueOnce([]).mockResolvedValueOnce(["Open Settings"]);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });

    await fireEvent.keyDown(document.body, { key: ",", code: "Comma", ctrlKey: true });
    await vi.waitFor(() => {
      expect(core.recentCommands).toHaveBeenCalledTimes(2);
    });
    await fireEvent.keyDown(window, { key: "Escape" });
    await palette();

    expect(titles()[0]).toBe("Open Settings");
  });

  it("leaves the Repository picker in its own order, and a switch there out of the history", async () => {
    core.recentCommands.mockResolvedValue(["Add repository…", "python/peps"]);
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });

    await fireEvent.keyDown(window, { key: "r", code: "KeyR", ctrlKey: true });
    const listed = options();
    await fireEvent.click(screen.getByRole("option", { name: "python/peps" }));

    expect(listed).toEqual(["Home", "some-maintainer/gcmon open", "python/peps", "Add repository…"]);
    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(core.rememberCommand).not.toHaveBeenCalled();
  });
});

describe("every action in the palette", () => {
  /** Each of the palette's rows: its text, and the key on its right, or null for none. */
  const rows = () =>
    screen
      .getAllByRole("option")
      .map((option) => [rowText(option), option.querySelector("kbd")?.textContent ?? null]);

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
  });

  afterEach(() => {
    remapKeys({});
  });

  /** The page open on gcmon with the focus on #7's row. */
  const onRow = async () => {
    core.workbench.mockResolvedValue(oneIssue);
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });
    expect(document.activeElement).toBe(row);
    return row;
  };

  it("lists the actions that run in the Issue list, each with its key, in the key table's order", async () => {
    await onRow();
    await ctrlShiftP();
    // The diff's actions wait for a Files tab, and the actions with nothing built behind them yet, such as
    // Local Mark, for theirs. The palette's own keys open the box it is.
    expect(rows()).toEqual([
      ["Hide Sidebar", "Ctrl+B"],
      ["Hide Issue List", "Ctrl+Alt+B"],
      ["Next Pane", "F6"],
      ["Previous Pane", "Shift+F6"],
      ["Unfold an Issue's PRs", "→"],
      ["Fold an Issue's PRs", "←"],
      ["Show Only the Detail Pane", "Ctrl+K Z"],
      ["Pin or Unpin", "Ctrl+K P"],
      ["Pick a Theme", "Ctrl+K T"],
      ["Reload Themes", "Ctrl+K R"],
      ["Go Back", "Alt+←"],
      ["Go Forward", "Alt+→"],
      ["Open on GitHub", "Ctrl+Shift+G"],
      ["Switch Repository", "Ctrl+R"],
      ["Go to Home", null],
      ["Reset Layout", null],
      ["Archive", "E"],
      ["Mute", "M"],
      ["Mute Author", null],
      ["Mark Seen", "Q"],
      ["Mark Unseen", "Shift+Q"],
      ["Open Settings", "Ctrl+,"],
      ["Show performance HUD", "Ctrl+Shift+H"],
      ["List the View's Keys", "?"],
      ["Sort by Last Update", null],
      ["View: Inbox", null],
      ["View: Stale Reviews", null],
      ["View: Archived", null],
      ["View: Pinned Issues", null],
      ["View: Sidebar as Icons", null],
    ]);
  });

  it("lists the diff's actions while the Files tab shows", async () => {
    listOnePr();
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "#70 gh-7: Fix the crash" }));
    const files = await screen.findByRole("tab", { name: "Files" });
    await fireEvent.click(files);
    files.focus();
    await screen.findByRole("tab", { name: "Files", selected: true });
    await ctrlShiftP();
    // The focus is in the Detail pane, so the list's ← and → have nothing to fold.
    expect(rows()).toEqual([
      ["Go to Definition", "F12"],
      ["Peek Definition", "Alt+F12"],
      ["Next Change", "F7"],
      ["Previous Change", "Shift+F7"],
      ["Next Comment Thread", "F8"],
      ["Previous Comment Thread", "Shift+F8"],
      ["Find in Diff", "Ctrl+F"],
      ["Hide Sidebar", "Ctrl+B"],
      ["Hide Issue List", "Ctrl+Alt+B"],
      ["Next Pane", "F6"],
      ["Previous Pane", "Shift+F6"],
      ["Show Only the Detail Pane", "Ctrl+K Z"],
      ["Unified or Side by Side", "Ctrl+Alt+U"],
      ["View: One File at a Time", "Ctrl+K O"],
      ["View: Files in the Rail", "Ctrl+K F"],
      ["View: Search in the Rail", "Ctrl+Shift+F"],
      ["Next Tab", "Ctrl+PageDown"],
      ["Previous Tab", "Ctrl+PageUp"],
      ["Next File", "Alt+PageDown"],
      ["Previous File", "Alt+PageUp"],
      ["Pick a Theme", "Ctrl+K T"],
      ["Reload Themes", "Ctrl+K R"],
      ["Go Back", "Alt+←"],
      ["Go Forward", "Alt+→"],
      ["Open on GitHub", "Ctrl+Shift+G"],
      ["Switch Repository", "Ctrl+R"],
      ["Go to Home", null],
      ["Reset Layout", null],
      ["Archive", "E"],
      ["Mute", "M"],
      ["Mute Author", null],
      ["Mark Seen", "Q"],
      ["Mark Unseen", "Shift+Q"],
      ["Open Settings", "Ctrl+,"],
      ["Show performance HUD", "Ctrl+Shift+H"],
      ["List the View's Keys", "?"],
      ["Sort by Last Update", null],
      ["View: Inbox", null],
      ["View: Stale Reviews", null],
      ["View: Archived", null],
      ["View: Pinned Issues", null],
      ["View: Sidebar as Icons", null],
    ]);
  });

  it("runs what the key runs, for the focus the palette hands back", async () => {
    await onRow();
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "List the View's Keys" }));

    // The palette has handed the focus back to the row by then, so the list names the Issue list's keys.
    const keys = await screen.findByRole("dialog", { name: "Keys" });
    expect(
      within(keys)
        .getAllByRole("region")
        .map((section) => section.getAttribute("aria-label")),
    ).toEqual(["Issue list", "Anywhere"]);
  });

  it("runs a diff's action, as its key does", async () => {
    listOnePr();
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "#70 gh-7: Fix the crash" }));
    await fireEvent.click(await screen.findByRole("tab", { name: "Files" }));
    await screen.findByRole("tab", { name: "Files", selected: true });
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Find in Diff" }));

    expect(await screen.findByRole("search", { name: "Find in the diff" })).toBeTruthy();
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Find" }));
    });
  });

  it("names the key on each row to a screen reader, and shows a key remapped in [keys] by its new binding", async () => {
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { open_settings: "ctrl+k s", reset_layout: "ctrl+k l", pick_theme: "" };
    core.settings.mockResolvedValue(remapped);
    await onRow();
    await vi.waitFor(() => {
      expect(actionOn("ctrl+k s")).toBe("open_settings");
    });
    await ctrlShiftP();

    const keyOf = (title: string) => rows().find(([text]) => text === title)?.[1];
    expect(["Open Settings", "Reset Layout", "Pick a Theme"].map(keyOf)).toEqual([
      "Ctrl+K S",
      "Ctrl+K L",
      null,
    ]);
    expect(screen.getByRole("option", { name: "Open Settings", description: "Ctrl+K S" })).toBeTruthy();
  });

  it("runs Reload Themes from the Issue list and from the Files tab, as Ctrl+K R does", async () => {
    listOnePr();
    render(Page);
    const prRow = await screen.findByRole("button", { name: "#70 gh-7: Fix the crash" });
    await vi.waitFor(() => {
      expect(core.themes).toHaveBeenCalledOnce();
    });
    prRow.focus();
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Reload Themes" }));
    await fireEvent.click(prRow);
    const files = await screen.findByRole("tab", { name: "Files" });
    await fireEvent.click(files);
    files.focus();
    await screen.findByRole("tab", { name: "Files", selected: true });
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Reload Themes" }));

    await vi.waitFor(() => {
      expect(core.themes).toHaveBeenCalledTimes(3);
    });
  });

  // The palette's rows are worked out again as what they show changes, here the performance HUD's title,
  // and the row that had the focus may have left the page by then.
  it("keeps the Issue list's actions once the row that had the focus has left the page", async () => {
    const row = await onRow();
    await ctrlShiftP();
    row.remove();
    await fireEvent.keyDown(window, { key: "H", code: "KeyH", ctrlKey: true, shiftKey: true });
    const redrawn = rowTexts().includes("Hide performance HUD");
    await fireEvent.input(screen.getByRole("combobox", { name: "Command" }), { target: { value: ">fold" } });

    expect([redrawn, rowTexts()]).toEqual([true, ["Unfold an Issue's PRs", "Fold an Issue's PRs"]]);
  });
});

describe("Add repository on a Repository Tsugiki has", () => {
  /** Opens Add repository, and checks a name the core says Tsugiki has as `name`. */
  const checkHeld = async (name: string) => {
    core.checkRepository.mockResolvedValue({ status: "ok", data: { kind: "held", name } });
    await openAddFromPicker();
    await fireEvent.click(screen.getByRole("button", { name: "Check" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Switch to it" }));
  };

  beforeEach(() => {
    core.home.mockResolvedValue(gcmonAfterPeps);
  });

  it("switches to it as the palette does, and adds nothing", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    render(Page);
    await checkHeld(peps.name);

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(core.openRepository).toHaveBeenCalledWith("python/peps");
    expect(core.addRepository).not.toHaveBeenCalled();
    await vi.waitFor(() => {
      expect(core.settings).toHaveBeenCalledTimes(2);
    });
  });

  it("closes without a switch on the open Repository", async () => {
    render(Page);
    await checkHeld(gcmon.name);

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(core.openRepository).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: gcmon.name })).toBeTruthy();
  });
});

describe("the Repository picker", () => {
  const ctrlR = (target: Element = document.body) =>
    fireEvent.keyDown(target, { key: "r", code: "KeyR", ctrlKey: true });
  const picker = () => screen.queryByRole("dialog", { name: "Repository picker" });
  const filter = () => screen.getByRole("combobox", { name: "Repository" });
  /** The window open on gcmon, with peps opened before it and a/third before that. */
  const threeHome = openOn(gcmon, [peps.name, "a/third"]);

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(threeHome);
  });

  it("opens on Ctrl+R: Home, the Repositories most recently opened first, the open one marked, the previous picked", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();

    expect(picker()).toBeTruthy();
    expect(document.activeElement).toBe(filter());
    expect(options()).toEqual([
      "Home",
      "some-maintainer/gcmon open",
      "python/peps",
      "a/third",
      "Add repository…",
    ]);
    expect(spaced(screen.getByRole("option", { current: true }))).toBe("some-maintainer/gcmon open");
    expect(screen.getByRole("option", { selected: true }).textContent).toBe("python/peps");
  });

  it("notes why a Repository's diffs aren't there yet, and follows the clone while it is open", async () => {
    core.mirrorStates.mockResolvedValue([
      { repository: gcmon.name, state: { state: "ready" } },
      { repository: peps.name, state: { state: "cloning", progress: { stage: "receiving", percent: 40 } } },
      { repository: "a/third", state: { state: "waiting", behind: peps.name } },
    ]);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    // The page asks for the states as it loads, after a few other answers.
    await vi.waitFor(() => {
      expect(options()).toEqual([
        "Home",
        "some-maintainer/gcmon open",
        "python/peps · cloning, 40%",
        "a/third · waiting for python/peps's clone",
        "Add repository…",
      ]);
    });

    mirrorStates.changed([
      { repository: gcmon.name, state: { state: "ready" } },
      { repository: peps.name, state: { state: "ready" } },
      { repository: "a/third", state: { state: "cloning", progress: { stage: "starting" } } },
    ]);
    await vi.waitFor(() => {
      expect(options()).toEqual([
        "Home",
        "some-maintainer/gcmon open",
        "python/peps",
        "a/third · cloning",
        "Add repository…",
      ]);
    });
    // The note is no part of the name, so typing doesn't match it.
    await fireEvent.input(filter(), { target: { value: "cloning" } });
    expect(screen.queryAllByRole("option")).toEqual([]);
  });

  it("notes the open Repository's mirror too", async () => {
    core.mirrorStates.mockResolvedValue([{ repository: gcmon.name, state: { state: "asking" } }]);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await vi.waitFor(() => {
      expect(options()[1]).toBe("some-maintainer/gcmon · the mirror waits for your answer open");
    });
  });

  it("picks the open Repository when it is the only one", async () => {
    core.home.mockResolvedValue(gcmonHome);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    expect(spaced(screen.getByRole("option", { selected: true }))).toBe("some-maintainer/gcmon open");
  });

  it("switches back to the previous Repository on Ctrl+R and Enter", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(core.openRepository).toHaveBeenCalledWith("python/peps");
    // The core read the files again to switch, so a problem the user has since fixed can go.
    await vi.waitFor(() => {
      expect(core.settings).toHaveBeenCalledTimes(2);
    });
  });

  it("filters the rows as the user types", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await fireEvent.input(filter(), { target: { value: "PEP" } });
    expect(options()).toEqual(["python/peps"]);
  });

  it("opens Add repository from its row, which gives the focus back to what had it before the picker", async () => {
    render(Page);
    const gear = await screen.findByRole("button", { name: "Settings" });
    gear.focus();
    await ctrlR(gear);
    await fireEvent.input(filter(), { target: { value: "add" } });
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    const dialog = await screen.findByRole("dialog", { name: "Add repository" });
    // The picker gives the focus back as it closes, and the dialog takes it after.
    expect(document.activeElement).toBe(within(dialog).getByRole("textbox", { name: "Repository" }));

    await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(gear);
  });

  it("closes without a switch on Enter on the open Repository", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await fireEvent.keyDown(filter(), { key: "ArrowUp" });
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    expect(core.openRepository).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: gcmon.name })).toBeTruthy();
  });

  it("closes on Esc and gives the focus back to what had it", async () => {
    render(Page);
    const gear = await screen.findByRole("button", { name: "Settings" });
    gear.focus();
    await ctrlR(gear);
    expect(document.activeElement).toBe(filter());

    await fireEvent.keyDown(filter(), { key: "Escape" });
    expect(picker()).toBeNull();
    expect(document.activeElement).toBe(gear);
  });

  it("opens from the Repository's name atop the Sidebar", async () => {
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: gcmon.name }));
    expect(picker()).toBeTruthy();
  });

  it("opens on Ctrl+R with the Sidebar hidden", async () => {
    render(Page);
    await screen.findByRole("navigation", { name: "Sidebar" });
    await fireEvent.keyDown(document.body, { key: "b", code: "KeyB", ctrlKey: true });
    await vi.waitFor(() => {
      expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
    });
    await ctrlR();
    expect(picker()).toBeTruthy();
  });

  it("doesn't open on onboarding", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await ctrlR();
    expect(picker()).toBeNull();
  });

  it("doesn't open over the add dialog", async () => {
    render(Page);
    await openAddFromPicker();
    await ctrlR();
    expect(picker()).toBeNull();
  });

  it("and the command palette don't open over each other", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await ctrlShiftP();
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(picker()).toBeTruthy();

    await fireEvent.keyDown(window, { key: "Escape" });
    await ctrlShiftP();
    await ctrlR();
    expect(picker()).toBeNull();
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
  });
});

describe("the Home page", () => {
  const HOUR_MS = 3_600_000;
  /** A time `hours` before now, as the core writes it. */
  const hoursAgo = (hours: number) => new Date(Date.now() - hours * HOUR_MS).toISOString();
  /** The window open on gcmon, opened an hour ago, with peps opened before it and a/never not opened here. */
  const homeOfThree = () =>
    openOn(gcmon, [{ name: peps.name, opened_at: hoursAgo(26) }, "a/never"], hoursAgo(1));
  const ctrlR = () => fireEvent.keyDown(document.body, { key: "r", code: "KeyR", ctrlKey: true });
  const homeHeading = () => screen.queryByRole("heading", { name: "Home" });
  const repositoryRows = () =>
    within(screen.getByRole("list", { name: "Repositories" }))
      .getAllByRole("button")
      .map((row) => Array.from(row.querySelectorAll("span"), (span) => span.textContent));
  const rowOf = (name: string) =>
    within(screen.getByRole("list", { name: "Repositories" })).getByRole("button", {
      name: new RegExp(name),
    });

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(homeOfThree());
  });

  /** Opens the page on gcmon, and the Home page from the picker's first row. */
  const goHome = async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlR();
    await fireEvent.click(screen.getByRole("option", { name: "Home" }));
    await vi.waitFor(() => {
      expect(homeHeading()).toBeTruthy();
    });
  };

  it("opens from the picker's first row on every Repository, the one the user came from first", async () => {
    await goHome();

    expect(repositoryRows()).toEqual([
      ["some-maintainer/gcmon", "opened 1 h ago"],
      ["python/peps", "opened 1 d ago"],
      ["a/never"],
    ]);
    expect(document.activeElement).toBe(rowOf(gcmon.name));
    expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
    expect(screen.getByRole("status")).toBeTruthy();
    expect(document.title).toBe("Home · Tsugiki 1.2.3");
  });

  it("opens from Go to Home in the palette, whose rows there open a Repository or add one", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Go to Home" }));
    await vi.waitFor(() => {
      expect(homeHeading()).toBeTruthy();
    });

    await ctrlShiftP();
    const listed = rowTexts();
    await fireEvent.keyDown(window, { key: "Escape" });
    await fireEvent.keyDown(window, { key: "p", code: "KeyP", ctrlKey: true });

    expect(listed).toContain("Open Settings");
    expect(listed).not.toContain("Go to Home");
    expect(listed.slice(-4)).toEqual([
      "Repository: some-maintainer/gcmon",
      "Repository: python/peps",
      "Repository: a/never",
      "Add repository…",
    ]);
    // Quick open searches one Repository, so the Home page has none.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(homeHeading()).toBeTruthy();
  });

  it("lists on ? the window's keys, Go Back among them, and none of the Repository's screen it hides", async () => {
    await goHome();
    await fireEvent.keyDown(document.activeElement as Element, { key: "?", code: "Slash", shiftKey: true });
    const listed = within(await screen.findByRole("dialog", { name: "Keys" }))
      .getAllByRole("definition")
      .map((title) => title.textContent);
    expect(listed).toEqual(expect.arrayContaining(["Go Back", "Switch Repository", "Open Settings"]));
    expect(listed).not.toContain("Hide or Show the Sidebar");
  });

  it("keeps Add repository… in its palette out of the history, as the Repository rows beside it", async () => {
    await goHome();
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Add repository…" }));
    expect(await screen.findByRole("dialog", { name: "Add repository" })).toBeTruthy();
    expect(core.rememberCommand).not.toHaveBeenCalled();
  });

  it("goes back to the Repository the user came from on Enter on its row, Esc or Alt+←, with no switch", async () => {
    await goHome();
    // A browser clicks the focused row on Enter, which jsdom leaves to the test.
    await fireEvent.keyDown(document.activeElement as Element, { key: "Enter" });
    await fireEvent.click(document.activeElement as Element);
    expect(await screen.findByRole("navigation", { name: "Sidebar" })).toBeTruthy();
    expect(document.title).toBe("some-maintainer/gcmon · Tsugiki 1.2.3");

    await ctrlR();
    await fireEvent.click(screen.getByRole("option", { name: "Home" }));
    await vi.waitFor(() => {
      expect(homeHeading()).toBeTruthy();
    });
    await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(await screen.findByRole("navigation", { name: "Sidebar" })).toBeTruthy();

    await ctrlR();
    await fireEvent.click(screen.getByRole("option", { name: "Home" }));
    await vi.waitFor(() => {
      expect(homeHeading()).toBeTruthy();
    });
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowLeft", altKey: true });
    expect(await screen.findByRole("navigation", { name: "Sidebar" })).toBeTruthy();
    expect(homeHeading()).toBeNull();
    expect(core.openRepository).not.toHaveBeenCalled();
  });

  it("opens the Repository under the cursor on Enter, after ↑ ↓ move it", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    await goHome();
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowUp" });
    expect(document.activeElement).toBe(rowOf(peps.name));
    expect(rowOf(peps.name).getAttribute("aria-current")).toBe("true");

    // Nothing holds the focus after a click beside the rows, and Enter still opens the row under the cursor.
    (document.activeElement as HTMLElement).blur();
    await fireEvent.keyDown(document.body, { key: "Enter" });

    expect(core.openRepository).toHaveBeenCalledWith(peps.name);
    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(homeHeading()).toBeNull();
  });

  it("opens a Repository on a click on its row", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    await goHome();
    await fireEvent.click(rowOf(peps.name));
    expect(await screen.findByRole("button", { name: peps.name })).toBeTruthy();
    expect(homeHeading()).toBeNull();
  });

  it("stays and says why when the core refuses the switch", async () => {
    core.openRepository.mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "no disk" },
    });
    await goHome();
    await fireEvent.click(rowOf(peps.name));
    expect(await screen.findByText("Tsugiki couldn't switch to python/peps: no disk")).toBeTruthy();
    expect(homeHeading()).toBeTruthy();
  });

  it("keeps the cursor on a row when a refused switch finds the last row's Repository gone", async () => {
    core.openRepository.mockResolvedValueOnce({
      status: "error",
      error: { kind: "not_found", message: "Tsugiki doesn't have a/never." },
    });
    await goHome();
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    await fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    core.home.mockResolvedValue(openOn(gcmon, [{ name: peps.name, opened_at: hoursAgo(26) }], hoursAgo(1)));
    await fireEvent.click(rowOf("a/never"));
    await vi.waitFor(() => {
      expect(repositoryRows()).toEqual([
        ["some-maintainer/gcmon", "opened 1 h ago"],
        ["python/peps", "opened 1 d ago"],
      ]);
    });

    expect(rowOf(peps.name).getAttribute("aria-current")).toBe("true");
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    await fireEvent.keyDown(document.body, { key: "Enter" });
    expect(core.openRepository).toHaveBeenLastCalledWith(peps.name);
  });

  it("has the picker on Ctrl+R, with Home marked and the Repository the user came from picked", async () => {
    await goHome();
    await ctrlR();
    expect(screen.getAllByRole("option").map(spaced)).toEqual([
      "Home open",
      "some-maintainer/gcmon",
      "python/peps",
      "a/never",
      "Add repository…",
    ]);
    expect(screen.getByRole("option", { selected: true }).textContent).toBe(gcmon.name);

    await fireEvent.keyDown(screen.getByRole("combobox", { name: "Repository" }), { key: "Enter" });
    expect(await screen.findByRole("navigation", { name: "Sidebar" })).toBeTruthy();
    expect(core.openRepository).not.toHaveBeenCalled();
  });

  it("keeps its keys under a dialog: Esc closes the dialog and leaves the page on show", async () => {
    await goHome();
    await fireEvent.click(screen.getByRole("button", { name: "Add repository…" }));
    const dialog = await screen.findByRole("dialog", { name: "Add repository" });
    await fireEvent.keyDown(within(dialog).getByRole("textbox", { name: "Repository" }), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(homeHeading()).toBeTruthy();
  });

  it("goes back to the open Repository when Add repository finds Tsugiki has it", async () => {
    core.checkRepository.mockResolvedValue({ status: "ok", data: { kind: "held", name: gcmon.name } });
    await goHome();
    await fireEvent.click(screen.getByRole("button", { name: "Add repository…" }));
    const dialog = await screen.findByRole("dialog", { name: "Add repository" });
    await fireEvent.input(within(dialog).getByRole("textbox", { name: "Repository" }), {
      target: { value: gcmon.name },
    });
    await fireEvent.click(within(dialog).getByRole("button", { name: "Check" }));
    await fireEvent.click(await within(dialog).findByRole("button", { name: "Switch to it" }));
    expect(await screen.findByRole("navigation", { name: "Sidebar" })).toBeTruthy();
    expect(core.openRepository).not.toHaveBeenCalled();
  });
});

describe("a chord", () => {
  const chord = () => screen.queryByText("Ctrl+K");

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
  });

  it("waits after Ctrl+K, with the status bar saying so, and the next key ends it doing nothing else", async () => {
    core.workbench.mockResolvedValue(oneIssue);
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });

    await ctrlK();
    expect(chord()).toBeTruthy();
    await fireEvent.keyDown(document.body, { key: "ArrowDown", code: "ArrowDown" });

    expect(chord()).toBeNull();
    expect(document.activeElement).not.toBe(row);
  });

  it("hands a chord of Go Back to the Repository's screen, as Go Back is the page's only on the Home page", async () => {
    listOnePr();
    core.workbench.mockResolvedValue(
      workbenchPage([
        listedIssue(7, { title: "It crashes" }),
        { kind: "standalone_pr", ...pr, backports: [], attention: null },
      ]),
    );
    core.openIssue.mockResolvedValue(done);
    core.issue.mockResolvedValue(ok(null));
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { go_back: "ctrl+k g" };
    core.settings.mockResolvedValue(remapped);
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "#7 It crashes" }));
    await screen.findByRole("region", { name: "Issue view" });
    await fireEvent.click(screen.getByRole("button", { name: "#70 gh-7: Fix the crash" }));
    await screen.findByRole("region", { name: "PR view" });
    await vi.waitFor(() => {
      expect(actionOn("ctrl+k g")).toBe("go_back");
    });

    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "g", code: "KeyG" });

    expect(await screen.findByRole("region", { name: "Issue view" })).toBeTruthy();
  });

  it("hands Ctrl+K Z to the Repository's screen, which shows only the Detail pane", async () => {
    render(Page);
    await screen.findByRole("navigation", { name: "Sidebar" });
    expect(document.querySelector('[data-pane="list"]')).not.toBeNull();

    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "z", code: "KeyZ" });

    expect(chord()).toBeNull();
    expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
    expect(document.querySelector('[data-pane="list"]')).toBeNull();
  });

  it("ends at a click", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlK();
    expect(chord()).toBeTruthy();
    await fireEvent.pointerDown(document.body);
    expect(chord()).toBeNull();
  });

  it("ends when the window loses the focus", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlK();
    expect(chord()).toBeTruthy();
    await fireEvent.blur(window);
    expect(chord()).toBeNull();
  });

  it("doesn't start under the add dialog, even outside its text box", async () => {
    render(Page);
    await openAddFromPicker();
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("doesn't start under the palette", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await ctrlShiftP();
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("doesn't start under the Repository picker", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await fireEvent.keyDown(window, { key: "r", code: "KeyR", ctrlKey: true });
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("starts on onboarding for the page's own chords, as Ctrl+K T picks a theme there", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "t", code: "KeyT" });
    expect(screen.queryByRole("dialog", { name: "Theme picker" })).toBeTruthy();
  });
});

describe("the theme picker", () => {
  const ctrlKT = async () => {
    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "t", code: "KeyT" });
  };
  const themePicker = () => screen.queryByRole("dialog", { name: "Theme picker" });
  const bg = () => document.documentElement.style.getPropertyValue("--bg");

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
  });

  /** The page on gcmon, with the theme in force on the window. */
  const opened = async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await vi.waitFor(() => {
      expect(bg()).toBe("#1f1f1f");
    });
  };

  it("opens on Ctrl+K T on Follow system, previews on the window, and Esc puts the theme back", async () => {
    await opened();
    await ctrlKT();

    expect(themePicker()).toBeTruthy();
    expect(options()).toEqual([
      "Follow system chosen",
      "Light",
      "Dark",
      "Gruvbox Dark Hard",
      "Reload themes",
    ]);
    await fireEvent.keyDown(screen.getByRole("combobox", { name: "Theme" }), { key: "ArrowDown" });
    expect(bg()).toBe("#ffffff");
    await fireEvent.keyDown(window, { key: "Escape" });

    expect(themePicker()).toBeNull();
    expect(bg()).toBe("#1f1f1f");
    expect(core.saveTheme).not.toHaveBeenCalled();
  });

  it("opens from the palette, and Enter saves the theme, which stays on the window", async () => {
    await opened();
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Pick a Theme" }));
    const box = await screen.findByRole("combobox", { name: "Theme" });
    const reads = core.settings.mock.calls.length;

    for (let i = 0; i < 3; i++) await fireEvent.keyDown(box, { key: "ArrowDown" });
    await fireEvent.keyDown(box, { key: "Enter" });

    await vi.waitFor(() => {
      expect(core.saveTheme).toHaveBeenCalledExactlyOnceWith({ kind: "named", name: "Gruvbox Dark Hard" });
    });
    expect(themePicker()).toBeNull();
    expect(bg()).toBe("#1d2021");
    // The save read the files again, so a problem the user has since fixed can go.
    await vi.waitFor(() => {
      expect(core.settings.mock.calls.length).toBe(reads + 1);
    });
  });

  it("says in the status bar why a theme couldn't be saved, and puts the old one back", async () => {
    core.saveTheme.mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "settings.toml, line 1: expected `]`" },
    });
    await opened();
    await ctrlKT();
    const box = screen.getByRole("combobox", { name: "Theme" });
    await fireEvent.keyDown(box, { key: "ArrowDown" });
    await fireEvent.keyDown(box, { key: "Enter" });

    await vi.waitFor(() => {
      expect(screen.getByRole("status").textContent).toContain(
        "Tsugiki can't save the theme: settings.toml, line 1: expected `]`",
      );
    });
    expect(bg()).toBe("#1f1f1f");
  });

  it("says in the status bar why the title bar couldn't turn, and shows the theme all the same", async () => {
    core.turnWindow.mockRejectedValue(new Error("the window has gone"));
    await opened();

    await vi.waitFor(() => {
      expect(screen.getByRole("status").textContent).toContain(
        "Tsugiki can't turn its title bar to the theme's side: Error: the window has gone",
      );
    });
  });

  /** The core's answer from now on: THEMES with a dark theme file named `name`, and `problems`. */
  const withFile = (name: string, problems: string[] = []) => {
    const file = { name, kind: "dark", tokens: [{ property: "--bg", value: "#002b36" }] };
    const data = { ...THEMES, dark: { ...THEMES.dark, all: [...THEMES.dark.all, file] }, problems };
    core.themes.mockResolvedValue({ status: "ok", data });
  };

  it("reads the themes folder again as it opens, so a file added since is listed", async () => {
    await opened();
    withFile("Solarized Dark");
    await ctrlKT();

    await vi.waitFor(() => {
      expect(options()).toContain("Solarized Dark");
    });
  });

  it("reads the themes folder again on Ctrl+K R and from the palette, and names a file's problems", async () => {
    const problem = "themes\\mine.toml, line 6: fg isn't a colour, so Tsugiki skips it.";
    await opened();
    const reads = core.themes.mock.calls.length;
    withFile("Mine", [problem]);
    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "r", code: "KeyR" });

    await vi.waitFor(() => {
      expect(screen.getByRole("status").textContent).toContain(problem);
    });
    expect(core.themes).toHaveBeenCalledTimes(reads + 1);
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: /^Reload Themes/ }));
    await vi.waitFor(() => {
      expect(core.themes).toHaveBeenCalledTimes(reads + 2);
    });
  });

  it("keeps the palette, the Repository picker and the chords from opening over it", async () => {
    await opened();
    await ctrlKT();
    await ctrlShiftP();
    await fireEvent.keyDown(document.body, { key: "r", code: "KeyR", ctrlKey: true });
    await ctrlK();

    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Repository picker" })).toBeNull();
    expect(screen.queryByText("Ctrl+K")).toBeNull();
    expect(themePicker()).toBeTruthy();
  });
});

describe("the Settings dialog", () => {
  const ctrlComma = (target: Element = document.body) =>
    fireEvent.keyDown(target, { key: ",", code: "Comma", ctrlKey: true });
  const settings = () => screen.queryByRole("dialog", { name: "Settings" });
  /** The Account page's value beside "Signed in as". */
  const signedInAs = () =>
    screen.getByText("Signed in as", { selector: "dt" }).nextElementSibling?.textContent.trim();

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
    core.workbench.mockResolvedValue(oneIssue);
  });

  /** The page on gcmon, with the focus on its one row. */
  const onRow = async () => {
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });
    expect(document.activeElement).toBe(row);
    return row;
  };

  it("opens on Ctrl+, on its General page; its Account page names who is signed in; Esc gives the focus back", async () => {
    const row = await onRow();
    await ctrlComma(row);
    expect(document.activeElement).toBe(settings());
    expect(screen.getByRole("region", { name: "General" })).toBeTruthy();
    const pages = screen.getByRole("navigation", { name: "Settings pages" });
    await fireEvent.click(within(pages).getByRole("button", { name: "Account" }));
    expect(screen.getByRole("region", { name: "Account" })).toBeTruthy();
    // The status comes when the core has answered each of the window's first questions.
    await vi.waitFor(() => {
      expect(signedInAs()).toBe("octocat");
    });
    expect(await screen.findByText("gh", { selector: "b" })).toBeTruthy();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(settings()).toBeNull();
    expect(document.activeElement).toBe(row);
  });

  it("hides the status bar at once when its box is cleared, Cancel brings it back, and Apply keeps it hidden", async () => {
    const row = await onRow();
    await screen.findByRole("button", { name: "Not synced yet" });
    const statusBarBox = () => screen.getByRole("checkbox", { name: "Show the status bar" });
    const clearBox = async () => {
      await ctrlComma(row);
      await vi.waitFor(() => {
        expect(statusBarBox().hasAttribute("disabled")).toBe(false);
      });
      await fireEvent.click(statusBarBox());
      expect(screen.queryByRole("button", { name: "Not synced yet" })).toBeNull();
    };

    await clearBox();
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(settings()).toBeNull();
    expect(screen.getByRole("button", { name: "Not synced yet" })).toBeTruthy();

    await clearBox();
    await fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await vi.waitFor(() => {
      expect(settings()).toBeNull();
    });
    expect(screen.queryByRole("button", { name: "Not synced yet" })).toBeNull();
    expect(core.applySettings).toHaveBeenCalledOnce();
    expect(core.applySettings).toHaveBeenCalledWith({ status_bar: { visible: false } });
  });

  it("shows a hand edit made since start-up as it opens, and the window takes it", async () => {
    const hidden = structuredClone(SETTINGS);
    hidden.settings.status_bar.visible = false;
    core.rereadSettings.mockResolvedValue({ status: "ok", data: hidden });
    const row = await onRow();
    await screen.findByRole("button", { name: "Not synced yet" });
    const themeReads = core.themes.mock.calls.length;

    await ctrlComma(row);
    await vi.waitFor(() => {
      expect(screen.getByRole("checkbox", { name: "Show the status bar" })).toHaveProperty("checked", false);
    });
    expect(screen.queryByRole("button", { name: "Not synced yet" })).toBeNull();
    expect(screen.getByRole("button", { name: "OK" })).toBeTruthy();
    // The theme, which the window holds apart from the settings, takes a hand edit to it too.
    expect(core.themes).toHaveBeenCalledTimes(themeReads + 1);
  });

  it("opens on Advanced at the row the Definition peek's link asks for, and on General again from Ctrl+,", async () => {
    const row = await onRow();
    settingsAsk.open("clangd");
    await vi.waitFor(() => {
      expect(screen.getByRole("region", { name: "Advanced" })).toBeTruthy();
    });
    expect(screen.getByRole("group", { name: "clangd" }).classList.contains("asked")).toBe(true);
    expect(settingsAsk.asked).toBeUndefined();
    await fireEvent.keyDown(window, { key: "Escape" });

    await ctrlComma(row);
    expect(screen.getByRole("region", { name: "General" })).toBeTruthy();
  });

  it("drops the peek's ask for Settings while another dialog is open", async () => {
    await onRow();
    await ctrlShiftP();
    settingsAsk.open("ty");
    await vi.waitFor(() => {
      expect(settingsAsk.asked).toBeUndefined();
    });
    expect(settings()).toBeNull();
  });

  it("opens from ⚙ in the Sidebar, and gives the focus back to it", async () => {
    render(Page);
    const gear = await screen.findByRole("button", { name: "Settings" });
    // jsdom doesn't focus a button it clicks, as a browser does.
    gear.focus();
    await fireEvent.click(gear);
    expect(settings()).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(settings()).toBeNull();
    expect(document.activeElement).toBe(gear);
  });

  it("opens from ⚙ Settings on onboarding", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(settings()).toBeTruthy();
  });

  it("opens on Ctrl+, on onboarding", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByRole("button", { name: "Add repository" });
    await ctrlComma();
    expect(settings()).toBeTruthy();
  });

  it("opens from the palette, and gives the focus back to what had it before the palette", async () => {
    const row = await onRow();
    await ctrlShiftP();
    await fireEvent.click(screen.getByRole("option", { name: "Open Settings" }));
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(settings());
    });

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(document.activeElement).toBe(row);
  });

  it("keeps the list's keys, the palette, the Repository picker and the chords from working under it", async () => {
    const row = await onRow();
    await ctrlComma(row);
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });
    await ctrlShiftP();
    await fireEvent.keyDown(document.body, { key: "r", code: "KeyR", ctrlKey: true });
    await ctrlK();

    expect(document.activeElement).toBe(settings());
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Repository picker" })).toBeNull();
    expect(screen.queryByText("Ctrl+K")).toBeNull();
  });

  it("doesn't open over another dialog", async () => {
    await onRow();
    await ctrlShiftP();
    await ctrlComma(screen.getByRole("combobox", { name: "Command" }));
    expect(settings()).toBeNull();
  });

  it("keeps a button behind it from opening another dialog, so one Esc closes only it", async () => {
    core.home.mockResolvedValue(onboarding);
    core.mirrorQuestion.mockResolvedValue("python/cpython");
    render(Page);
    const add = await screen.findByRole("button", { name: "Add repository" });
    const question = await screen.findByRole("button", {
      name: "python/cpython's mirror waits for your answer",
    });
    await ctrlComma();

    await fireEvent.click(add);
    await fireEvent.click(question);
    expect(screen.queryByRole("dialog", { name: "Add repository" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Mirror of python/cpython" })).toBeNull();
    expect(settings()).toBeTruthy();
  });
});

describe("the key list", () => {
  const question = (target: Element = document.body) =>
    fireEvent.keyDown(target, { key: "?", code: "Slash", shiftKey: true });
  const keyList = () => screen.queryByRole("dialog", { name: "Keys" });
  const sections = () =>
    within(keyList() as HTMLElement)
      .getAllByRole("region")
      .flatMap((section) => section.getAttribute("aria-label") ?? []);

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  });

  afterEach(() => {
    remapKeys({});
  });

  it("opens on ? with the Issue list's keys, and closes on Esc with the focus back", async () => {
    core.home.mockResolvedValue(gcmonHome);
    core.workbench.mockResolvedValue(oneIssue);
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });

    await question(row);
    const list = keyList();
    expect(list).toBeTruthy();
    expect(document.activeElement).toBe(list);
    expect(sections()).toEqual(["Issue list", "Anywhere"]);
    const issueList = within(within(list as HTMLElement).getByRole("region", { name: "Issue list" }));
    expect(issueList.getByText("Unfold an Issue's PRs").previousElementSibling?.textContent).toBe("→");
    // Archive and Mute act on the Detail pane's item too, as Q does.
    const anywhere = within(screen.getByRole("region", { name: "Anywhere" }));
    expect(anywhere.getByText("Archive").previousElementSibling?.textContent).toBe("E");
    expect(anywhere.getByText("Mute").previousElementSibling?.textContent).toBe("M");
    // The list's keys stay under it.
    await fireEvent.keyDown(window, { key: "ArrowDown" });

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(keyList()).toBeNull();
    expect(document.activeElement).toBe(row);
  });

  it("shows a key remapped in [keys] by its new binding", async () => {
    core.home.mockResolvedValue(gcmonHome);
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { mute: "a" };
    core.settings.mockResolvedValue(remapped);
    // Mute is listed while it has an item to act on.
    core.workbench.mockResolvedValue(oneIssue);
    render(Page);
    const row = await screen.findByRole("button", { name: "#7 It crashes" });
    await fireEvent.keyDown(document.body, { key: "ArrowDown" });
    await vi.waitFor(() => {
      expect(actionOn("a")).toBe("mute");
    });

    await question(row);
    const anywhere = within(screen.getByRole("region", { name: "Anywhere" }));
    expect(anywhere.getByText("Mute").previousElementSibling?.textContent).toBe("A");
  });

  /** The titles of the actions the list names. */
  const titles = () => screen.getAllByRole("definition").map((title) => title.textContent);

  // Switch Repository has no Repository to switch from.
  it("lists only the page's own keys on onboarding, which has no Repository's screen", async () => {
    render(Page);
    await screen.findByText(/Your token comes from/);
    await question();

    expect(sections()).toEqual(["Anywhere"]);
    expect(titles()).toEqual([
      "Command Palette",
      "Pick a Theme",
      "Reload Themes",
      "Open Settings",
      "Show or Hide the Performance HUD",
      "List the View's Keys",
    ]);
  });

  it("leaves out a key a chord's wait takes, and lists it where no chord starts on it", async () => {
    core.home.mockResolvedValue(gcmonHome);
    const remapped = structuredClone(SETTINGS);
    // Ctrl+K starts the screen's chords, such as Ctrl+K Z, so the wait takes it, but none in a text box.
    remapped.settings.keys = {
      open_settings: "ctrl+k",
      pick_theme: "",
      reload_themes: "",
      show_keys: "ctrl+/",
    };
    core.settings.mockResolvedValue(remapped);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await vi.waitFor(() => {
      expect(actionOn("ctrl+k")).toBe("open_settings");
    });
    const ctrlSlash = (target: Element) =>
      fireEvent.keyDown(target, { key: "/", code: "Slash", ctrlKey: true });
    await ctrlSlash(document.body);
    const onScreen = titles();
    await fireEvent.keyDown(window, { key: "Escape" });
    const box = document.createElement("input");
    document.body.append(box);
    await ctrlSlash(box);
    box.remove();

    expect([onScreen.includes("Open Settings"), titles().includes("Open Settings")]).toEqual([false, true]);
  });

  it("lists in a text box only the keys that work there", async () => {
    core.home.mockResolvedValue(gcmonHome);
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { show_keys: "ctrl+/", toggle_hud: "h", reload_themes: "k r" };
    core.settings.mockResolvedValue(remapped);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await vi.waitFor(() => {
      expect(actionOn("ctrl+/")).toBe("show_keys");
    });
    const box = document.createElement("input");
    document.body.append(box);
    await fireEvent.keyDown(box, { key: "/", code: "Slash", ctrlKey: true });
    box.remove();

    // H and K type there, and a chord of the screen's, such as Ctrl+K Z, doesn't start in a text box.
    expect(titles()).toEqual(
      expect.arrayContaining(["Command Palette", "Hide or Show the Sidebar", "Pick a Theme"]),
    );
    for (const title of [
      "Show or Hide the Performance HUD",
      "Reload Themes",
      "Show Only the Detail Pane, or the Panes",
      "Go Back",
    ]) {
      expect(titles()).not.toContain(title);
    }
  });

  it("lists Open DevTools in a development build, whose webview opens DevTools on any screen", async () => {
    core.about.mockResolvedValue({
      name: "Tsugiki",
      version: "1.2.3",
      build: "development",
      source_ref: "main",
    });
    render(Page);
    await vi.waitFor(() => {
      expect(document.title).toBe("Tsugiki 1.2.3");
    });
    await question();
    expect(titles()).toContain("Open DevTools");
  });

  it("lists Go Back on a Repository's screen, which runs it through its history", async () => {
    core.home.mockResolvedValue(gcmonHome);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await question();
    expect(titles()).toContain("Go Back");
  });

  it("leaves Open DevTools out of a release", async () => {
    core.home.mockResolvedValue(gcmonHome);
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await question();
    expect(titles()).not.toContain("Open DevTools");
  });

  it("types ? in a text box", async () => {
    render(Page);
    await screen.findByText(/Your token comes from/);
    const box = document.createElement("input");
    document.body.append(box);
    const typed = await question(box);
    box.remove();

    expect([typed, keyList()]).toEqual([true, null]);
  });

  it("doesn't open over another dialog", async () => {
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { show_keys: "ctrl+/" };
    core.settings.mockResolvedValue(remapped);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await vi.waitFor(() => {
      expect(actionOn("ctrl+/")).toBe("show_keys");
    });
    await ctrlShiftP();
    // Ctrl+/ types nothing, so the palette's text box doesn't keep it.
    await fireEvent.keyDown(screen.getByRole("combobox", { name: "Command" }), {
      key: "/",
      code: "Slash",
      ctrlKey: true,
    });
    expect(keyList()).toBeNull();
  });
});

describe("keys remapped in [keys]", () => {
  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonHome);
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { pick_theme: "ctrl+t", command_palette: "ctrl+k c" };
    core.settings.mockResolvedValue(remapped);
  });

  afterEach(() => {
    remapKeys({});
  });

  it("runs a chord's action on a key of one press, and a key of one press's action on a chord", async () => {
    render(Page);
    await screen.findByRole("button", { name: gcmon.name });
    await vi.waitFor(() => {
      expect(actionOn("ctrl+t")).toBe("pick_theme");
    });

    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "c", code: "KeyC" });
    const palette = screen.queryByRole("dialog", { name: "Command palette" });
    // Behind the palette, Pick a Theme does nothing, as a chord can't start there.
    await fireEvent.keyDown(document.body, { key: "t", code: "KeyT", ctrlKey: true });
    const behind = screen.queryByRole("dialog", { name: "Theme picker" });
    await fireEvent.keyDown(window, { key: "Escape" });
    await fireEvent.keyDown(document.body, { key: "t", code: "KeyT", ctrlKey: true });

    expect([palette !== null, behind]).toEqual([true, null]);
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Theme picker" })).toBeTruthy();
  });

  it("runs the page's actions on a chord wherever their keys of one press work, and the screen's nowhere else", async () => {
    core.home.mockResolvedValue(onboarding);
    const remapped = structuredClone(SETTINGS);
    remapped.settings.keys = { open_settings: "ctrl+k s", toggle_detail_only: "ctrl+j z" };
    core.settings.mockResolvedValue(remapped);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await vi.waitFor(() => {
      expect(actionOn("ctrl+k s")).toBe("open_settings");
    });
    const box = document.createElement("input");
    document.body.append(box);

    // A chord of the screen's starts no wait here, or Ctrl+K would end it, and S do nothing.
    await fireEvent.keyDown(document.body, { key: "j", code: "KeyJ", ctrlKey: true });
    await fireEvent.keyDown(box, { key: "k", code: "KeyK", ctrlKey: true });
    await fireEvent.keyDown(box, { key: "s", code: "KeyS" });
    box.remove();

    expect(screen.queryByRole("dialog", { name: "Settings" })).toBeTruthy();
  });
});
