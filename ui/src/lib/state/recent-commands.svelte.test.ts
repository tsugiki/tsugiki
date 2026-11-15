import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { RecentCommands } from "./recent-commands.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { recentCommands: vi.fn(), rememberCommand: vi.fn() } }));

const core = vi.mocked(commands);

/** Rows titled as given, in that order. */
const rows = (...titles: string[]) => titles.map((title) => ({ title }));
const titles = (ordered: { title: string }[]) => ordered.map((row) => row.title);

beforeEach(() => {
  vi.resetAllMocks();
  core.recentCommands.mockResolvedValue([]);
  core.rememberCommand.mockResolvedValue({ status: "ok", data: [] });
});

it("lists the rows run, most recently run first, then the others in their own order", async () => {
  core.recentCommands.mockResolvedValue(["Reload Themes", "Open Settings", "Gone"]);
  const recent = new RecentCommands();
  const before = titles(recent.order(rows("Pick a Theme", "Open Settings", "Reset Layout", "Reload Themes")));

  await recent.load();

  expect(before).toEqual(["Pick a Theme", "Open Settings", "Reset Layout", "Reload Themes"]);
  expect(
    titles(recent.order(rows("Pick a Theme", "Open Settings", "Reset Layout", "Reload Themes"))),
  ).toEqual(["Reload Themes", "Open Settings", "Pick a Theme", "Reset Layout"]);
});

it("puts a row run first, and asks the core to save it", async () => {
  const recent = new RecentCommands();
  await recent.load();

  await recent.ran({ title: "Open Settings" });
  await recent.ran({ title: "Reset Layout" });
  await recent.ran({ title: "Open Settings" });

  expect(titles(recent.order(rows("Reset Layout", "Pick a Theme", "Open Settings")))).toEqual([
    "Open Settings",
    "Reset Layout",
    "Pick a Theme",
  ]);
  expect(core.rememberCommand.mock.calls).toEqual([["Open Settings"], ["Reset Layout"], ["Open Settings"]]);
  expect(recent.problem).toBeUndefined();
});

it("knows a row by its id, which stays as its title changes", async () => {
  const recent = new RecentCommands();

  await recent.ran({ title: "Hide Sidebar", id: "toggle_sidebar" });
  const ordered = recent.order([{ title: "Reset Layout" }, { title: "Show Sidebar", id: "toggle_sidebar" }]);

  expect(titles(ordered)).toEqual(["Show Sidebar", "Reset Layout"]);
  expect(core.rememberCommand).toHaveBeenCalledWith("toggle_sidebar");
});

it("keeps a row run before the core answers ahead of what the core read", async () => {
  let answer: (ids: string[]) => void = () => {};
  core.recentCommands.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const recent = new RecentCommands();
  const loading = recent.load();

  await recent.ran({ title: "Reset Layout" });
  answer(["Open Settings", "Reset Layout"]);
  await loading;

  expect(titles(recent.order(rows("Open Settings", "Pick a Theme", "Reset Layout")))).toEqual([
    "Reset Layout",
    "Open Settings",
    "Pick a Theme",
  ]);
});

it("saves in the order the rows ran, each once the save before has finished", async () => {
  const saved: string[] = [];
  let finish: () => void = () => {};
  core.rememberCommand.mockImplementationOnce(
    (id) =>
      new Promise((resolve) => {
        finish = () => {
          saved.push(id);
          resolve({ status: "ok", data: [] });
        };
      }),
  );
  core.rememberCommand.mockImplementation(async (id) => {
    saved.push(id);
    return { status: "ok", data: [] };
  });
  const recent = new RecentCommands();

  const first = recent.ran({ title: "Open Settings" });
  const second = recent.ran({ title: "Reset Layout" });
  await vi.waitFor(() => {
    expect(core.rememberCommand).toHaveBeenCalledOnce();
  });
  finish();
  await Promise.all([first, second]);

  expect(saved).toEqual(["Open Settings", "Reset Layout"]);
});

it("says why the history can't be read, and keeps the rows' own order", async () => {
  core.recentCommands.mockRejectedValue(new Error("the core stopped"));
  const recent = new RecentCommands();

  await recent.load();

  expect(recent.problem).toBe("Tsugiki can't read the palette's recent commands: Error: the core stopped");
  expect(titles(recent.order(rows("Pick a Theme", "Open Settings")))).toEqual([
    "Pick a Theme",
    "Open Settings",
  ]);
});

it("says why a run can't be saved, keeps it first for now, and forgets the problem once a save works", async () => {
  core.rememberCommand.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "machine.toml:1: expected `]`" },
  });
  const recent = new RecentCommands();

  await recent.ran({ title: "Reset Layout" });
  const problem = recent.problem;
  const ordered = titles(recent.order(rows("Open Settings", "Reset Layout")));
  await recent.ran({ title: "Open Settings" });

  expect(problem).toBe("Tsugiki can't save the palette's recent commands: machine.toml:1: expected `]`");
  expect(ordered).toEqual(["Reset Layout", "Open Settings"]);
  expect(recent.problem).toBeUndefined();
});
