import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { PanesState } from "./panes.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { paneWidths: vi.fn(), savePaneWidths: vi.fn() } }));

const core = vi.mocked(commands);

beforeEach(() => {
  vi.resetAllMocks();
});

it("takes the defaults until the core says what the user dragged", async () => {
  core.paneWidths.mockResolvedValue({ sidebar: 240, list: null });
  const panes = new PanesState();
  const before = [panes.sidebar, panes.list];

  await panes.load();

  expect(before).toEqual([null, null]);
  expect([panes.sidebar, panes.list, panes.problem]).toEqual([240, null, undefined]);
});

it("keeps widths written by hand within each pane's limits", async () => {
  const panes = new PanesState();
  core.paneWidths.mockResolvedValueOnce({ sidebar: 100_000, list: 3 });
  await panes.load();
  const wide = [panes.sidebar, panes.list];
  const again = new PanesState();
  core.paneWidths.mockResolvedValueOnce({ sidebar: 3, list: null });

  await again.load();

  expect([wide, [again.sidebar, again.list]]).toEqual([
    [480, 280],
    [140, null],
  ]);
});

it("keeps a width the user dragged before the core answered", async () => {
  core.paneWidths.mockResolvedValue({ sidebar: 240, list: 500 });
  const panes = new PanesState();
  const loading = panes.load();
  panes.sidebar = 300;

  await loading;

  expect([panes.sidebar, panes.list]).toEqual([300, 500]);
});

it("keeps the defaults and says why when it can't ask, after a save that works too", async () => {
  core.paneWidths.mockRejectedValue(new Error("the core has gone"));
  core.savePaneWidths.mockResolvedValue({ status: "ok", data: { sidebar: null, list: null } });
  const panes = new PanesState();

  await panes.load();
  await panes.save();

  expect([panes.sidebar, panes.list]).toEqual([null, null]);
  expect(panes.problem).toBe("Tsugiki can't read the panes' widths: Error: the core has gone");
});

it("sends a save only once the one before has finished, so the file ends with the last widths", async () => {
  let finish: () => void = () => {};
  core.savePaneWidths
    .mockReturnValueOnce(
      new Promise((resolve) => {
        finish = () => {
          resolve({ status: "ok", data: { sidebar: 240, list: null } });
        };
      }),
    )
    .mockResolvedValue({ status: "ok", data: { sidebar: null, list: null } });
  const panes = new PanesState();
  panes.sidebar = 240;
  const first = panes.save();
  // The first save has gone out to the core by the time the user resets the pane.
  await Promise.resolve();
  const reset = panes.reset("sidebar");
  await Promise.resolve();
  const meanwhile = core.savePaneWidths.mock.calls.length;

  finish();
  await Promise.all([first, reset]);

  expect(meanwhile).toBe(1);
  expect(core.savePaneWidths.mock.calls).toEqual([
    [{ sidebar: 240, list: null }],
    [{ sidebar: null, list: null }],
  ]);
});

it("saves both widths, and a reset saves the default", async () => {
  core.savePaneWidths.mockResolvedValue({ status: "ok", data: { sidebar: 240, list: 500 } });
  const panes = new PanesState();
  panes.sidebar = 240;
  panes.list = 500;

  await panes.save();
  await panes.reset("list");

  expect(core.savePaneWidths.mock.calls).toEqual([
    [{ sidebar: 240, list: 500 }],
    [{ sidebar: 240, list: null }],
  ]);
  expect([panes.list, panes.problem]).toEqual([null, undefined]);
});

it("says why it couldn't save, until a save works", async () => {
  const panes = new PanesState();
  core.savePaneWidths.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "locked" },
  });
  await panes.save();
  const refused = panes.problem;
  core.savePaneWidths.mockRejectedValueOnce(new Error("no core"));
  await panes.save();
  const rejected = panes.problem;

  core.savePaneWidths.mockResolvedValueOnce({ status: "ok", data: { sidebar: null, list: null } });
  await panes.save();

  expect([refused, rejected, panes.problem]).toEqual([
    "Tsugiki can't save the panes' widths: locked",
    "Tsugiki can't save the panes' widths: Error: no core",
    undefined,
  ]);
});
