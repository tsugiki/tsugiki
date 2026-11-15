import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { RecentCommands } from "./recent-commands.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { recentCommands: vi.fn(), rememberCommand: vi.fn() } }));

const core = vi.mocked(commands);

/** Rows titled as given, in that order. */
const rows = (...titles: string[]) => titles.map((title) => ({ title }));
const titles = (ordered: { title: string }[]) => ordered.map((row) => row.title);

/** What `machine.toml` holds, as the core's answers keep it. */
let kept: string[] = [];

beforeEach(() => {
  vi.resetAllMocks();
  kept = [];
  core.recentCommands.mockImplementation(async () => kept);
  core.rememberCommand.mockImplementation(async (id) => {
    kept = [id, ...kept.filter((one) => one !== id)];
    return { status: "ok", data: kept };
  });
});

it("lists the rows run, most recently run first, then the others in their own order", async () => {
  kept = ["Reload Themes", "Open Settings", "Gone"];
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

it("takes the core's answer as the list, which drops what the core no longer keeps", async () => {
  kept = ["Open Settings", "Pick a Theme"];
  const recent = new RecentCommands();
  await recent.load();
  core.rememberCommand.mockResolvedValueOnce({ status: "ok", data: ["Reset Layout", "Open Settings"] });

  await recent.ran({ title: "Reset Layout" });

  expect(titles(recent.order(rows("Pick a Theme", "Open Settings", "Reset Layout")))).toEqual([
    "Reset Layout",
    "Open Settings",
    "Pick a Theme",
  ]);
});

it("knows a row by its id, which stays as its title changes", async () => {
  const recent = new RecentCommands();

  await recent.ran({ title: "Hide Sidebar", id: "toggle_sidebar" });
  const ordered = recent.order([{ title: "Reset Layout" }, { title: "Show Sidebar", id: "toggle_sidebar" }]);

  expect(titles(ordered)).toEqual(["Show Sidebar", "Reset Layout"]);
  expect(core.rememberCommand).toHaveBeenCalledWith("toggle_sidebar");
});

it("shows a row first while its save waits, and saves without waiting for the read", async () => {
  core.recentCommands.mockReturnValue(new Promise(() => {}));
  let answer: () => void = () => {};
  core.rememberCommand.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = () => {
        resolve({ status: "ok", data: ["Reset Layout", "Open Settings"] });
      };
    }),
  );
  const recent = new RecentCommands();
  void recent.load();

  const saving = recent.ran({ title: "Reset Layout" });
  const waiting = titles(recent.order(rows("Open Settings", "Pick a Theme", "Reset Layout")));
  answer();
  await saving;

  expect(waiting).toEqual(["Reset Layout", "Open Settings", "Pick a Theme"]);
  expect(core.rememberCommand).toHaveBeenCalledWith("Reset Layout");
  expect(titles(recent.order(rows("Pick a Theme", "Open Settings", "Reset Layout")))).toEqual([
    "Reset Layout",
    "Open Settings",
    "Pick a Theme",
  ]);
});

it("takes a save's answer over a read that answers after it", async () => {
  let read: (ids: string[]) => void = () => {};
  core.recentCommands.mockReturnValue(
    new Promise((resolve) => {
      read = resolve;
    }),
  );
  const recent = new RecentCommands();
  const loading = recent.load();

  await recent.ran({ title: "Reset Layout" });
  read(["Open Settings"]);
  await loading;

  expect(titles(recent.order(rows("Open Settings", "Reset Layout")))).toEqual([
    "Reset Layout",
    "Open Settings",
  ]);
});

it("keeps each row run while the save before it waits in front of the core's answer to that one", async () => {
  let finish: () => void = () => {};
  core.rememberCommand.mockImplementationOnce(
    (id) =>
      new Promise((resolve) => {
        finish = () => {
          kept = [id];
          resolve({ status: "ok", data: kept });
        };
      }),
  );
  const recent = new RecentCommands();

  const first = recent.ran({ title: "Open Settings" });
  const second = recent.ran({ title: "Reset Layout" });
  await vi.waitFor(() => {
    expect(core.rememberCommand).toHaveBeenCalledOnce();
  });
  finish();
  await first;
  const between = titles(recent.order(rows("Pick a Theme", "Open Settings", "Reset Layout")));
  await second;

  expect(between).toEqual(["Reset Layout", "Open Settings", "Pick a Theme"]);
  expect(core.rememberCommand.mock.calls).toEqual([["Open Settings"], ["Reset Layout"]]);
  expect(kept).toEqual(["Reset Layout", "Open Settings"]);
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

it("forgets why the history couldn't be read once a save answers with the core's list", async () => {
  core.recentCommands.mockRejectedValueOnce(new Error("the core stopped"));
  const recent = new RecentCommands();
  await recent.load();
  const unread = recent.problem;

  await recent.ran({ title: "Reset Layout" });

  expect(unread).toBe("Tsugiki can't read the palette's recent commands: Error: the core stopped");
  expect(recent.problem).toBeUndefined();
});

it("forgets why the history couldn't be read once a later read works", async () => {
  core.recentCommands.mockRejectedValueOnce(new Error("the core stopped"));
  const recent = new RecentCommands();
  await recent.load();

  await recent.load();

  expect(recent.problem).toBeUndefined();
});

it("takes a read asked after a save answered, as after the Settings dialog read a hand edit", async () => {
  const recent = new RecentCommands();
  await recent.ran({ title: "Reset Layout" });
  kept = ["Pick a Theme"];

  await recent.load();

  expect(titles(recent.order(rows("Reset Layout", "Pick a Theme")))).toEqual([
    "Pick a Theme",
    "Reset Layout",
  ]);
});

it("says why a run can't be saved, puts it back where the core has it, and forgets the problem once a save works", async () => {
  kept = ["Open Settings"];
  const recent = new RecentCommands();
  await recent.load();
  core.rememberCommand.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "machine.toml:1: expected `]`" },
  });

  await recent.ran({ title: "Reset Layout" });
  const problem = recent.problem;
  const ordered = titles(recent.order(rows("Reset Layout", "Pick a Theme", "Open Settings")));
  await recent.ran({ title: "Pick a Theme" });

  expect(problem).toBe("Tsugiki can't save the palette's recent commands: machine.toml:1: expected `]`");
  expect(ordered).toEqual(["Open Settings", "Reset Layout", "Pick a Theme"]);
  expect(recent.problem).toBeUndefined();
});

it("goes on saving after a call that throws", async () => {
  core.rememberCommand.mockImplementationOnce(() => {
    throw new Error("no IPC");
  });
  const recent = new RecentCommands();

  await recent.ran({ title: "Reset Layout" });
  const problem = recent.problem;
  await recent.ran({ title: "Open Settings" });

  expect(problem).toBe("Tsugiki can't save the palette's recent commands: Error: no IPC");
  expect(core.rememberCommand.mock.calls).toEqual([["Reset Layout"], ["Open Settings"]]);
  expect(kept).toEqual(["Open Settings"]);
  expect(recent.problem).toBeUndefined();
});
