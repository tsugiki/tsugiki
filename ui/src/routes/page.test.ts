import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const core = vi.hoisted(() => ({
  settings: vi.fn(),
  home: vi.fn(),
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  openRepository: vi.fn(),
  workbench: vi.fn(),
  mirrorProblem: vi.fn(),
  checkMirrorsAgain: vi.fn(),
  mirrorQuestion: vi.fn(),
  mirrorStates: vi.fn(),
  chooseMirrorSource: vi.fn(),
  mirrorCost: vi.fn(),
}));

/** The listener for the mirrors' states, so a test can send it what the core would. */
const mirrorStates = vi.hoisted(() => ({ changed: vi.fn<(payload: unknown) => void>() }));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    about: async () => ({ name: "Tsugiki", version: "1.2.3" }),
    status: async () => ({ state: "online", login: "octocat" }),
    lastSync: async () => ({ repository: "some-maintainer/gcmon", at: null }),
    syncActivity: async () => null,
    cloning: async () => null,
    newVersion: async () => null,
    refresh: async () => null,
    themes: async () => ({
      status: "ok",
      data: {
        light: { name: "Light", kind: "light", tokens: [{ property: "--bg", value: "#ffffff" }] },
        dark: { name: "Dark", kind: "dark", tokens: [{ property: "--bg", value: "#1f1f1f" }] },
      },
    }),
    ...core,
  },
  events: {
    statusChanged: { listen: async () => () => {} },
    lastSyncChanged: { listen: async () => () => {} },
    syncActivityChanged: { listen: async () => () => {} },
    cloningChanged: { listen: async () => () => {} },
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
    changed: { listen: async () => () => {} },
    readFailed: { listen: async () => () => {} },
  },
}));

// jsdom has no `matchMedia`: the system stays on its dark side.
vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: () => {} }));

const onboarding = { screen: "onboarding", token: "gh" } as const;
const gcmon = { name: "some-maintainer/gcmon", conventions: "github", filters: [] } as const;
/** The window open on gcmon, the only Repository. */
const gcmonHome = { screen: "repository", repository: gcmon, others: [] } as const;
/** The Workbench with one Issue, #7, on it. */
const oneIssue = {
  status: "ok",
  data: {
    items: [
      {
        kind: "issue",
        number: 7,
        title: "It crashes",
        state: "OPEN",
        people: [],
        updated_at: "2026-11-04T10:00:00Z",
        pull_requests: [],
        attention: null,
      },
    ],
    quiet: [],
    count: 0,
    review_requests: 0,
    filters: [],
  },
} as const;

beforeEach(() => {
  // The tests share one document, so each starts with no title of its own.
  document.title = "";
  core.home.mockResolvedValue(onboarding);
  core.mirrorProblem.mockResolvedValue(null);
  core.mirrorQuestion.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.chooseMirrorSource.mockResolvedValue({ status: "ok", data: null });
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
  core.settings.mockResolvedValue({
    settings: { status_bar: { visible: true }, releases: { check: true }, repositories: [] },
    problems: [],
  });
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
  expect(document.title).toBe("Tsugiki");
});

it("reopens the Repository the core names, and the window title names it", async () => {
  core.home.mockResolvedValue(gcmonHome);
  render(Page);
  expect(await screen.findByRole("heading", { name: gcmon.name })).toBeTruthy();
  expect(document.title).toBe("some-maintainer/gcmon · Tsugiki");
});

it("says so when the core can't say what to open", async () => {
  core.home.mockRejectedValue(new Error("the core has gone"));
  render(Page);
  expect(await screen.findByText(/can't say what to open: Error: the core has gone/)).toBeTruthy();
  expect(document.title).toBe("Tsugiki");
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
        access: "other",
      },
    },
  });
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  expect(document.title).toBe("Tsugiki");
  await fireEvent.click(screen.getByRole("button", { name: "Check" }));
  await fireEvent.click(await screen.findByRole("button", { name: "Add" }));

  await screen.findByText(/Nothing here yet/);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("heading", { name: gcmon.name })).toBeTruthy();
  expect(document.title).toBe("some-maintainer/gcmon · Tsugiki");
  expect(core.settings).toHaveBeenCalledTimes(2);
});

it("keeps the palette from opening over the add dialog", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
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

it("asks the core when it opens and shows the answer", async () => {
  render(Page);
  expect(screen.getByText("Asking the core…")).toBeTruthy();
  expect(await screen.findByText("Tsugiki 1.2.3")).toBeTruthy();
});

it("shows who is signed in", async () => {
  render(Page);
  expect(await screen.findByText("octocat")).toBeTruthy();
});

it("opens the command palette on Ctrl+Shift+P and closes it on Esc", async () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  render(Page);
  expect(screen.queryByRole("dialog")).toBeNull();
  await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
  expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
  await fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("gives the list's keys to a dialog on top of it", async () => {
  core.home.mockResolvedValue(gcmonHome);
  core.workbench.mockResolvedValue(oneIssue);
  render(Page);
  const row = await screen.findByRole("button", { name: "#7 It crashes" });

  await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).not.toBe(row);

  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).toBe(row);
});

describe("the focus under the palette", () => {
  const ctrlShiftP = () =>
    fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
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
      await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
      await fireEvent.click(screen.getByRole("option", { name: title }));
    };
    await fromPalette("Show performance HUD");
    expect(panel()).toBeTruthy();
    await fromPalette("Hide performance HUD");
    expect(panel()).toBeNull();
  });
});

const peps = { name: "python/peps", conventions: "github", filters: [] } as const;
/** The window open on gcmon, with peps opened before it. */
const gcmonAfterPeps = { screen: "repository", repository: gcmon, others: [peps.name] } as const;
/** The window switched to peps. */
const pepsHome = { screen: "repository", repository: peps, others: [gcmon.name] } as const;
/** Each option's text, with the space before a row's "open" mark as a plain one. */
const spaced = (option: HTMLElement) => option.textContent.replace(/\s/g, " ");
const options = () => screen.getAllByRole("option").map(spaced);

describe("a switch from the palette", () => {
  const palette = async () => {
    await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
  };

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(gcmonAfterPeps);
  });

  it("offers each Repository but the open one", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
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

    expect(await screen.findByRole("heading", { name: peps.name })).toBeTruthy();
    expect(core.openRepository).toHaveBeenCalledWith("python/peps");
    expect(core.workbench).toHaveBeenLastCalledWith("python/peps", { kind: "whole" });
    expect(document.title).toBe("python/peps · Tsugiki");
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
    await screen.findByRole("heading", { name: gcmon.name });
    await palette();
    await fireEvent.click(screen.getByRole("option", { name: "Repository: python/peps" }));

    expect(
      await screen.findByText(
        "Tsugiki couldn't switch to python/peps: machine.toml, line 1: expected a value",
      ),
    ).toBeTruthy();
    expect(screen.getByRole("heading", { name: gcmon.name })).toBeTruthy();
    expect(document.title).toBe("some-maintainer/gcmon · Tsugiki");
  });
});

describe("Add repository on a Repository Tsugiki has", () => {
  /** Opens Add repository, and checks a name the core says Tsugiki has as `name`. */
  const checkHeld = async (name: string) => {
    core.checkRepository.mockResolvedValue({ status: "ok", data: { kind: "held", name } });
    await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
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
    expect(await screen.findByRole("heading", { name: peps.name })).toBeTruthy();
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
    expect(screen.getByRole("heading", { name: gcmon.name })).toBeTruthy();
  });
});

describe("the Repository picker", () => {
  const ctrlR = (target: Element = document.body) =>
    fireEvent.keyDown(target, { key: "r", code: "KeyR", ctrlKey: true });
  const picker = () => screen.queryByRole("dialog", { name: "Repository picker" });
  const filter = () => screen.getByRole("combobox", { name: "Repository" });
  /** The window open on gcmon, with peps opened before it and a/third before that. */
  const threeHome = { ...gcmonAfterPeps, others: [peps.name, "a/third"] };

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue(threeHome);
  });

  it("opens on Ctrl+R: the Repositories most recently opened first, the open one marked, the previous picked", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();

    expect(picker()).toBeTruthy();
    expect(document.activeElement).toBe(filter());
    expect(options()).toEqual(["some-maintainer/gcmon open", "python/peps", "a/third", "Add repository…"]);
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
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    // The page asks for the states as it loads, after a few other answers.
    await vi.waitFor(() => {
      expect(options()).toEqual([
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
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    await vi.waitFor(() => {
      expect(options()[0]).toBe("some-maintainer/gcmon · the mirror waits for your answer open");
    });
  });

  it("picks the open Repository when it is the only one", async () => {
    core.home.mockResolvedValue(gcmonHome);
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    expect(spaced(screen.getByRole("option", { selected: true }))).toBe("some-maintainer/gcmon open");
  });

  it("switches back to the previous Repository on Ctrl+R and Enter", async () => {
    core.openRepository.mockResolvedValue({ status: "ok", data: pepsHome });
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    expect(await screen.findByRole("heading", { name: peps.name })).toBeTruthy();
    expect(core.openRepository).toHaveBeenCalledWith("python/peps");
    // The core read the files again to switch, so a problem the user has since fixed can go.
    await vi.waitFor(() => {
      expect(core.settings).toHaveBeenCalledTimes(2);
    });
  });

  it("filters the rows as the user types", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    await fireEvent.input(filter(), { target: { value: "PEP" } });
    expect(options()).toEqual(["python/peps"]);
  });

  it("opens Add repository from its row, which gives the focus back to what had it before the picker", async () => {
    render(Page);
    const add = await screen.findByRole("button", { name: "Add repository" });
    add.focus();
    await ctrlR(add);
    await fireEvent.input(filter(), { target: { value: "add" } });
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    const dialog = await screen.findByRole("dialog", { name: "Add repository" });
    // The picker gives the focus back as it closes, and the dialog takes it after.
    expect(document.activeElement).toBe(within(dialog).getByRole("textbox", { name: "Repository" }));

    await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(add);
  });

  it("closes without a switch on Enter on the open Repository", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    await fireEvent.keyDown(filter(), { key: "ArrowUp" });
    await fireEvent.keyDown(filter(), { key: "Enter" });

    expect(picker()).toBeNull();
    expect(core.openRepository).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: gcmon.name })).toBeTruthy();
  });

  it("closes on Esc and gives the focus back to what had it", async () => {
    render(Page);
    const add = await screen.findByRole("button", { name: "Add repository" });
    add.focus();
    await ctrlR(add);
    expect(document.activeElement).toBe(filter());

    await fireEvent.keyDown(filter(), { key: "Escape" });
    expect(picker()).toBeNull();
    expect(document.activeElement).toBe(add);
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
    await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
    await ctrlR();
    expect(picker()).toBeNull();
  });

  it("and the command palette don't open over each other", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlR();
    await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(picker()).toBeTruthy();

    await fireEvent.keyDown(window, { key: "Escape" });
    await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
    await ctrlR();
    expect(picker()).toBeNull();
    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeTruthy();
  });
});

describe("a chord", () => {
  const ctrlK = () => fireEvent.keyDown(document.body, { key: "k", code: "KeyK", ctrlKey: true });
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

  it("hands Ctrl+K Z to the Repository's screen, which shows only the Detail pane", async () => {
    render(Page);
    await screen.findByRole("navigation", { name: "Sidebar" });

    await ctrlK();
    await fireEvent.keyDown(document.body, { key: "z", code: "KeyZ" });

    expect(chord()).toBeNull();
    expect(screen.queryByRole("navigation", { name: "Sidebar" })).toBeNull();
    expect(screen.queryByRole("heading", { name: gcmon.name })).toBeNull();
  });

  it("ends at a click", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlK();
    expect(chord()).toBeTruthy();
    await fireEvent.pointerDown(document.body);
    expect(chord()).toBeNull();
  });

  it("ends when the window loses the focus", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await ctrlK();
    expect(chord()).toBeTruthy();
    await fireEvent.blur(window);
    expect(chord()).toBeNull();
  });

  it("doesn't start under the add dialog, even outside its text box", async () => {
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("doesn't start under the palette", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("doesn't start under the Repository picker", async () => {
    render(Page);
    await screen.findByRole("heading", { name: gcmon.name });
    await fireEvent.keyDown(window, { key: "r", code: "KeyR", ctrlKey: true });
    await ctrlK();
    expect(chord()).toBeNull();
  });

  it("doesn't start on onboarding", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await ctrlK();
    expect(chord()).toBeNull();
  });
});
