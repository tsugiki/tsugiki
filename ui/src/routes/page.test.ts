import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const core = vi.hoisted(() => ({
  settings: vi.fn(),
  home: vi.fn(),
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  workbench: vi.fn(),
  mirrorProblem: vi.fn(),
  checkMirrorsAgain: vi.fn(),
  mirrorQuestion: vi.fn(),
  chooseMirrorSource: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    about: async () => ({ name: "Tsugiki", version: "1.2.3" }),
    status: async () => ({ state: "online", login: "octocat" }),
    lastSync: async () => ({ repository: "some-maintainer/gcmon", at: null }),
    syncActivity: async () => null,
    cloning: async () => null,
    refresh: async () => null,
    ...core,
  },
  events: {
    statusChanged: { listen: async () => () => {} },
    lastSyncChanged: { listen: async () => () => {} },
    syncActivityChanged: { listen: async () => () => {} },
    cloningChanged: { listen: async () => () => {} },
    mirrorProblemChanged: { listen: async () => () => {} },
    mirrorQuestionChanged: { listen: async () => () => {} },
    changed: { listen: async () => () => {} },
    readFailed: { listen: async () => () => {} },
  },
}));

const onboarding = { screen: "onboarding", token: "gh" } as const;
const gcmon = { name: "some-maintainer/gcmon", conventions: "github", filters: [] } as const;

beforeEach(() => {
  // The tests share one document, so each starts with no title of its own.
  document.title = "";
  core.home.mockResolvedValue(onboarding);
  core.mirrorProblem.mockResolvedValue(null);
  core.mirrorQuestion.mockResolvedValue(null);
  core.chooseMirrorSource.mockResolvedValue({ status: "ok", data: null });
  core.workbench.mockResolvedValue({
    status: "ok",
    data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] },
  });
  core.settings.mockResolvedValue({
    settings: { status_bar: { visible: true }, repositories: [] },
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

it("says why no mirror can be made, and a click asks the core to look again", async () => {
  core.mirrorProblem.mockResolvedValue({ kind: "no_git" });
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Check again" }));
  expect(screen.getByRole("status").textContent).toContain("can't find git");
  expect(core.checkMirrorsAgain).toHaveBeenCalledOnce();
});

it("asks where a mirror gets its objects, and hands the core the answer", async () => {
  core.mirrorQuestion.mockResolvedValue("python/cpython");
  render(Page);
  await fireEvent.click(await screen.findByRole("button", { name: "Download" }));
  expect(core.chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "download" });
});

it("opens on onboarding with no Repositories", async () => {
  render(Page);
  expect(await screen.findByText(/Your token comes from/)).toBeTruthy();
  expect(document.title).toBe("Tsugiki");
});

it("reopens the Repository the core names, and the window title names it", async () => {
  core.home.mockResolvedValue({ screen: "repository", repository: gcmon });
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
  const added = { screen: "repository", repository: gcmon } as const;
  core.addRepository.mockResolvedValue({ status: "ok", data: added });
  core.checkRepository.mockResolvedValue({
    status: "ok",
    data: {
      repository: gcmon,
      open_issues: 10,
      open_prs: 3,
      size: "small",
      window_days: 30,
      first_sync_items: 48,
      first_sync_seconds: 5,
      first_sync_points: 12,
      access: "other",
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
  core.home.mockResolvedValue({ screen: "repository", repository: gcmon });
  core.workbench.mockResolvedValue({
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
  });
  render(Page);
  const row = await screen.findByRole("button", { name: "#7 It crashes" });

  await fireEvent.keyDown(window, { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).not.toBe(row);

  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(document.activeElement).toBe(row);
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

describe("a chord", () => {
  const ctrlK = () => fireEvent.keyDown(document.body, { key: "k", code: "KeyK", ctrlKey: true });
  const chord = () => screen.queryByText("Ctrl+K");

  beforeEach(() => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    core.home.mockResolvedValue({ screen: "repository", repository: gcmon });
  });

  it("waits after Ctrl+K, with the status bar saying so, and the next key ends it doing nothing else", async () => {
    core.workbench.mockResolvedValue({
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
    });
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

  it("doesn't start on onboarding", async () => {
    core.home.mockResolvedValue(onboarding);
    render(Page);
    await screen.findByText(/Your token comes from/);
    await ctrlK();
    expect(chord()).toBeNull();
  });
});
