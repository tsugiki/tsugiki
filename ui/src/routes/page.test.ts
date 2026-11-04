import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const core = vi.hoisted(() => ({
  settings: vi.fn(),
  home: vi.fn(),
  checkRepository: vi.fn(),
  addRepository: vi.fn(),
  workbench: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    about: async () => ({ name: "Tsugiki", version: "1.2.3" }),
    status: async () => ({ state: "online", login: "octocat" }),
    ...core,
  },
  events: {
    statusChanged: { listen: async () => () => {} },
    workbenchChanged: { listen: async () => () => {} },
  },
}));

const onboarding = { screen: "onboarding", token: "gh" } as const;
const gcmon = { name: "sergey-miryanov/gcmon", conventions: "github", filters: [] } as const;

beforeEach(() => {
  core.home.mockResolvedValue(onboarding);
  core.workbench.mockResolvedValue({ status: "ok", data: { items: [], count: 0, filters: [] } });
  core.settings.mockResolvedValue({
    settings: { status_bar: { visible: true }, repositories: [] },
    problems: [],
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

it("opens on onboarding with no Repositories", async () => {
  render(Page);
  expect(await screen.findByText(/Your token comes from/)).toBeTruthy();
});

it("reopens the Repository the core names", async () => {
  core.home.mockResolvedValue({ screen: "repository", repository: gcmon });
  render(Page);
  expect(await screen.findByRole("heading", { name: gcmon.name })).toBeTruthy();
});

it("says so when the core can't say what to open", async () => {
  core.home.mockRejectedValue(new Error("the core has gone"));
  render(Page);
  expect(await screen.findByText(/can't say what to open: Error: the core has gone/)).toBeTruthy();
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
  await fireEvent.click(screen.getByRole("button", { name: "Check" }));
  await fireEvent.click(await screen.findByRole("button", { name: "Add" }));

  await screen.findByText(/Nothing here yet/);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("heading", { name: gcmon.name })).toBeTruthy();
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
