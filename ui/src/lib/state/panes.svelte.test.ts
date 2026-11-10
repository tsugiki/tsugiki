import { beforeEach, expect, it, vi } from "vitest";
import { commands, type PaneLayout_Serialize } from "$lib/data/bindings";
import { PanesState } from "./panes.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { paneLayout: vi.fn(), savePaneLayout: vi.fn() } }));

const core = vi.mocked(commands);
const DEFAULTS = {
  sidebar: null,
  list: null,
  sidebar_hidden: false,
  list_hidden: false,
  detail_only_hid: null,
};

beforeEach(() => {
  vi.resetAllMocks();
});

it("takes the defaults until the core says how the user laid out the panes", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, sidebar: 240, sidebar_hidden: true, list_hidden: true });
  const panes = new PanesState();
  const before = [panes.sidebar, panes.list, panes.hidden("sidebar"), panes.hidden("list")];

  await panes.load();

  expect(before).toEqual([null, null, false, false]);
  expect([panes.sidebar, panes.list, panes.hidden("sidebar"), panes.hidden("list"), panes.problem]).toEqual([
    240,
    null,
    true,
    true,
    undefined,
  ]);
});

it("keeps widths written by hand within each pane's limits", async () => {
  const panes = new PanesState();
  core.paneLayout.mockResolvedValueOnce({ ...DEFAULTS, sidebar: 100_000, list: 3 });
  await panes.load();
  const wide = [panes.sidebar, panes.list];
  const again = new PanesState();
  core.paneLayout.mockResolvedValueOnce({ ...DEFAULTS, sidebar: 3 });

  await again.load();

  expect([wide, [again.sidebar, again.list]]).toEqual([
    [480, 280],
    [140, null],
  ]);
});

it("keeps what the user changed before the core answered", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, sidebar: 240, list: 500, list_hidden: true });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  const loading = panes.load();
  panes.sidebar = 300;
  void panes.toggle("sidebar");
  void panes.show("list");

  await loading;

  expect([panes.sidebar, panes.list, panes.hidden("sidebar"), panes.hidden("list")]).toEqual([
    300,
    500,
    true,
    false,
  ]);
});

it("saves what the user changed before the core answered only once it has, with what the file holds", async () => {
  let answer: (layout: PaneLayout_Serialize) => void = () => {};
  core.paneLayout.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  const loading = panes.load();
  const hiding = panes.toggle("sidebar");
  await Promise.resolve();
  const meanwhile = core.savePaneLayout.mock.calls.length;

  answer({ ...DEFAULTS, sidebar: 240, list: 500 });
  await Promise.all([loading, hiding]);

  expect(meanwhile).toBe(0);
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULTS, sidebar: 240, list: 500, sidebar_hidden: true }],
  ]);
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
  await panes.toggle("sidebar");

  expect(thrown).toBe("Tsugiki can't save the panes' layout: Error: no IPC");
  expect([core.savePaneLayout.mock.calls.length, panes.problem]).toEqual([2, undefined]);
});

it("hides each pane and shows it again, saving each time with its width", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  panes.sidebar = 240;
  panes.list = 500;

  await panes.toggle("sidebar");
  await panes.toggle("list");
  const hidden = [panes.hidden("sidebar"), panes.hidden("list")];
  await panes.toggle("sidebar");
  await panes.toggle("list");

  expect([hidden, panes.hidden("sidebar"), panes.hidden("list")]).toEqual([[true, true], false, false]);
  const widths = { sidebar: 240, list: 500, detail_only_hid: null };
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...widths, sidebar_hidden: true, list_hidden: false }],
    [{ ...widths, sidebar_hidden: true, list_hidden: true }],
    [{ ...widths, sidebar_hidden: false, list_hidden: true }],
    [{ ...widths, sidebar_hidden: false, list_hidden: false }],
  ]);
});

it("shows a hidden pane and saves that, and leaves a pane on show as it is", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, list_hidden: true });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();

  await panes.show("list");
  await panes.show("list");
  await panes.show("sidebar");

  expect(panes.hidden("list")).toBe(false);
  expect(core.savePaneLayout.mock.calls).toEqual([[DEFAULTS]]);
});

it("keeps the defaults and says why when it can't ask, after a save that works too", async () => {
  core.paneLayout.mockRejectedValue(new Error("the core has gone"));
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();

  await panes.load();
  await panes.save();

  expect([panes.sidebar, panes.list, panes.hidden("sidebar"), panes.hidden("list")]).toEqual([
    null,
    null,
    false,
    false,
  ]);
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

it("puts the whole layout back to the defaults with one save, forgetting what Ctrl+K Z hid", async () => {
  core.paneLayout.mockResolvedValue({
    sidebar: 240,
    list: 500,
    sidebar_hidden: true,
    list_hidden: true,
    detail_only_hid: "list",
  });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();

  await panes.resetLayout();

  expect([panes.sidebar, panes.list, panes.hidden("sidebar"), panes.hidden("list")]).toEqual([
    null,
    null,
    false,
    false,
  ]);
  expect(core.savePaneLayout.mock.calls).toEqual([[DEFAULTS]]);
});

it("shows only the Detail pane, then brings back both panes, saving what it hid", async () => {
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();

  await panes.toggleDetailOnly();
  const hidden = [panes.hidden("sidebar"), panes.hidden("list"), panes.detailOnly];
  await panes.toggleDetailOnly();

  expect([hidden, panes.hidden("sidebar"), panes.hidden("list"), panes.detailOnly]).toEqual([
    [true, true, true],
    false,
    false,
    false,
  ]);
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULTS, sidebar_hidden: true, list_hidden: true, detail_only_hid: "both" }],
    [DEFAULTS],
  ]);
});

it("brings back only the Sidebar when the list was hidden already", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, list_hidden: true });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();

  await panes.toggleDetailOnly();
  await panes.toggleDetailOnly();

  expect([panes.hidden("sidebar"), panes.hidden("list")]).toEqual([false, true]);
  expect(core.savePaneLayout.mock.calls).toEqual([
    [{ ...DEFAULTS, sidebar_hidden: true, list_hidden: true, detail_only_hid: "sidebar" }],
    [{ ...DEFAULTS, list_hidden: true }],
  ]);
});

it("brings back only the list when the Sidebar was hidden already", async () => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, sidebar_hidden: true });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();

  await panes.toggleDetailOnly();
  const saved = core.savePaneLayout.mock.lastCall;
  await panes.toggleDetailOnly();

  expect(saved).toEqual([{ ...DEFAULTS, sidebar_hidden: true, list_hidden: true, detail_only_hid: "list" }]);
  expect([panes.hidden("sidebar"), panes.hidden("list")]).toEqual([true, false]);
});

it("brings back what the core says it hid before the restart", async () => {
  core.paneLayout.mockResolvedValue({
    ...DEFAULTS,
    sidebar_hidden: true,
    list_hidden: true,
    detail_only_hid: "list",
  });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();

  await panes.toggleDetailOnly();

  expect([panes.hidden("sidebar"), panes.hidden("list")]).toEqual([true, false]);
});

it.each([
  ["Ctrl+Alt+B", (panes: PanesState) => panes.toggle("list")],
  ["a click on a view", (panes: PanesState) => panes.show("list")],
])("forgets what it hid after %s, so it brings back both panes", async (_, change) => {
  core.paneLayout.mockResolvedValue({ ...DEFAULTS, list_hidden: true });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  await panes.load();
  await panes.toggleDetailOnly();

  await change(panes);
  await panes.toggle("list");
  await panes.toggleDetailOnly();

  expect([panes.hidden("sidebar"), panes.hidden("list")]).toEqual([false, false]);
  expect(core.savePaneLayout.mock.calls.at(1)).toEqual([{ ...DEFAULTS, sidebar_hidden: true }]);
});

it("forgets what it hid when the user hides a pane before the core answers", async () => {
  core.paneLayout.mockResolvedValue({
    ...DEFAULTS,
    sidebar_hidden: true,
    list_hidden: true,
    detail_only_hid: "list",
  });
  core.savePaneLayout.mockResolvedValue({ status: "ok", data: DEFAULTS });
  const panes = new PanesState();
  const loading = panes.load();
  void panes.toggle("sidebar");
  await loading;

  await panes.toggleDetailOnly();

  expect([panes.hidden("sidebar"), panes.hidden("list")]).toEqual([false, false]);
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
