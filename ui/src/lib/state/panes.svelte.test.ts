import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { PanesState } from "./panes.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { paneLayout: vi.fn(), savePaneLayout: vi.fn() } }));

const core = vi.mocked(commands);
const DEFAULTS = { sidebar: null, list: null, sidebar_hidden: false };

beforeEach(() => {
  vi.resetAllMocks();
});

it("takes the defaults until the core says how the user laid out the panes", async () => {
  core.paneLayout.mockResolvedValue({ sidebar: 240, list: null, sidebar_hidden: true });
  const panes = new PanesState();
  const before = [panes.sidebar, panes.list, panes.sidebarHidden];

  await panes.load();

  expect(before).toEqual([null, null, false]);
  expect([panes.sidebar, panes.list, panes.sidebarHidden, panes.problem]).toEqual([
    240,
    null,
    true,
    undefined,
  ]);
});

it("keeps widths written by hand within each pane's limits", async () => {
  const panes = new PanesState();
  core.paneLayout.mockResolvedValueOnce({ sidebar: 100_000, list: 3, sidebar_hidden: false });
  await panes.load();
  const wide = [panes.sidebar, panes.list];
  const again = new PanesState();
  core.paneLayout.mockResolvedValueOnce({ sidebar: 3, list: null, sidebar_hidden: false });

  await again.load();

  expect([wide, [again.sidebar, again.list]]).toEqual([
    [480, 280],
    [140, null],
  ]);
});

it("keeps what the user changed before the core answered", async () => {
  core.paneLayout.mockResolvedValue({ sidebar: 240, list: 500, sidebar_hidden: false });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  const loading = panes.load();
  panes.sidebar = 300;
  void panes.toggleSidebar();

  await loading;

  expect([panes.sidebar, panes.list, panes.sidebarHidden]).toEqual([300, 500, true]);
});

it("saves what the user changed before the core answered only once it has, with what the file holds", async () => {
  let answer: (layout: { sidebar: number; list: number; sidebar_hidden: boolean }) => void = () => {};
  core.paneLayout.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  const loading = panes.load();
  const hiding = panes.toggleSidebar();
  await Promise.resolve();
  const meanwhile = core.savePaneLayout.mock.calls.length;

  answer({ sidebar: 240, list: 500, sidebar_hidden: false });
  await Promise.all([loading, hiding]);

  expect(meanwhile).toBe(0);
  expect(core.savePaneLayout.mock.calls).toEqual([[{ sidebar: 240, list: 500, sidebar_hidden: true }]]);
});

it("saves again after a save that threw before it could answer", async () => {
  core.savePaneLayout
    .mockImplementationOnce(() => {
      throw new Error("no IPC");
    })
    .mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();

  await panes.save();
  const thrown = panes.problem;
  await panes.toggleSidebar();

  expect(thrown).toBe("Tsugiki can't save the panes' layout: Error: no IPC");
  expect([core.savePaneLayout.mock.calls.length, panes.problem]).toEqual([2, undefined]);
});

it("hides the Sidebar and shows it again, saving each time with its width", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  panes.sidebar = 240;

  await panes.toggleSidebar();
  const hidden = panes.sidebarHidden;
  await panes.toggleSidebar();

  expect([hidden, panes.sidebarHidden, panes.sidebar]).toEqual([true, false, 240]);
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ sidebar: 240, list: null, sidebar_hidden: true }],
    [{ sidebar: 240, list: null, sidebar_hidden: false }],
  ]);
});

it("keeps the defaults and says why when it can't ask, after a save that works too", async () => {
  core.paneLayout.mockRejectedValue(new Error("the core has gone"));
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();

  await panes.load();
  await panes.save();

  expect([panes.sidebar, panes.list, panes.sidebarHidden]).toEqual([null, null, false]);
  expect(panes.problem).toBe("Tsugiki can't read the panes' layout: Error: the core has gone");
});

it("sends a save only once the one before has finished, so the file ends with the last layout", async () => {
  let finish: () => void = () => {};
  core.savePaneLayout
    .mockReturnValueOnce(
      new Promise((resolve) => {
        finish = () => {
          resolve({ status: "ok", data: { ...DEFAULTS, sidebar: 240 } });
        };
      }),
    )
    .mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  panes.sidebar = 240;
  const first = panes.save();
  // The first save has gone out to the core by the time the user resets the pane.
  await Promise.resolve();
  const reset = panes.reset("sidebar");
  await Promise.resolve();
  const meanwhile = core.savePaneLayout.mock.calls.length;

  finish();
  await Promise.all([first, reset]);

  expect(meanwhile).toBe(1);
  expect(core.savePaneLayout.mock.calls).toEqual([[{ ...DEFAULTS, sidebar: 240 }], [DEFAULTS]]);
});

it("saves both widths, and a reset saves the default", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: { ...DEFAULTS, sidebar: 240, list: 500 } });
  const panes = new PanesState();
  panes.sidebar = 240;
  panes.list = 500;

  await panes.save();
  await panes.reset("list");

  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULTS, sidebar: 240, list: 500 }],
    [{ ...DEFAULTS, sidebar: 240 }],
  ]);
  expect([panes.list, panes.problem]).toEqual([null, undefined]);
});

it("says why it couldn't save, until a save works", async () => {
  const panes = new PanesState();
  core.savePaneLayout.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "locked" },
  });
  await panes.save();
  const refused = panes.problem;
  core.savePaneLayout.mockRejectedValueOnce(new Error("no core"));
  await panes.save();
  const rejected = panes.problem;

  core.savePaneLayout.mockResolvedValueOnce({ status: "ok", data: DEFAULTS });
  await panes.save();

  expect([refused, rejected, panes.problem]).toEqual([
    "Tsugiki can't save the panes' layout: locked",
    "Tsugiki can't save the panes' layout: Error: no core",
    undefined,
  ]);
});
