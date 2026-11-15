import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { keyTable, remapKeys } from "$lib/state/keymap.svelte";
import KeyboardPage from "./KeyboardPage.svelte";

/** The platform `navigator` reports, which says whether the keys read Ctrl or Cmd. */
let platform = "Win32";

beforeEach(() => {
  platform = "Win32";
  vi.spyOn(navigator, "platform", "get").mockImplementation(() => platform);
});

afterEach(() => {
  remapKeys({});
  vi.restoreAllMocks();
});

/** The table's rows below its header, each as its cells' text. */
const rows = () =>
  within(screen.getByRole("table"))
    .getAllByRole("row")
    .slice(1)
    .map((row) =>
      within(row)
        .getAllByRole("cell")
        .map((cell) => cell.textContent.trim()),
    );

/** The row of the action titled `title`, as its cells' text. */
const rowOf = (title: string) => rows().find(([action]) => action === title);

const filter = (text: string) =>
  fireEvent.input(screen.getByRole("searchbox", { name: "Filter keys" }), { target: { value: text } });

it("lists every action with its key and where it works, in the table's order", () => {
  render(KeyboardPage);

  expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).toEqual([
    "Action",
    "Key",
    "Where it works",
  ]);
  expect(rows().map(([action]) => action)).toEqual(keyTable().map((row) => row.title));
  expect(rowOf("Command Palette")).toEqual(["Command Palette", "Ctrl+Shift+P", "Anywhere"]);
  expect(rowOf("Next Change")).toEqual(["Next Change", "F7", "Diff"]);
  expect(rowOf("Archive")).toEqual(["Archive", "E", "Issue list"]);
  expect(rowOf("Send Comment")).toEqual(["Send Comment", "Ctrl+Enter", "Composer"]);
  expect(rowOf("Reset Layout")).toEqual(["Reset Layout", "No key", "Anywhere"]);
});

it("says the defaults follow VS Code, and that Cmd stands for Ctrl on macOS", () => {
  render(KeyboardPage);
  expect(screen.getByText("The default keys follow VS Code's. On macOS, Cmd stands for Ctrl.")).toBeTruthy();
});

it("leaves the rows whose action or key holds what the box holds, in any case", async () => {
  render(KeyboardPage);

  await filter("archive");
  expect(rows()).toEqual([["Archive", "E", "Issue list"]]);

  await filter("  CTRL+K ");
  const chords = rows().map(([, key]) => key);
  expect(chords).toContain("Ctrl+K M");
  expect(chords.length).toBeGreaterThan(5);
  expect(chords.every((key) => key?.startsWith("Ctrl+K "))).toBe(true);

  // By its name in `[keys]` too.
  await filter("local_mark");
  expect(rows()).toEqual([["Local Mark", "Ctrl+K M", "Anywhere"]]);

  await filter("");
  expect(rows()).toHaveLength(keyTable().length);
});

it("finds a key on macOS by Cmd, as the page shows it, and by Ctrl, as `[keys]` writes it", async () => {
  platform = "MacIntel";
  render(KeyboardPage);

  await filter("cmd+k m");
  expect(rows()).toEqual([["Local Mark", "Cmd+K M", "Anywhere"]]);
  await filter("ctrl+k m");
  expect(rows()).toEqual([["Local Mark", "Cmd+K M", "Anywhere"]]);
});

it("says so when no row holds what the box holds", async () => {
  render(KeyboardPage);
  await filter("no such action");
  expect(rows()).toEqual([["No action or key holds that."]]);
});

it("marks a remapped key, with its default, and says which action took a default key", () => {
  remapKeys({ archive: "f7", local_mark: "", reset_layout: "ctrl+k l" });
  render(KeyboardPage);

  expect(rowOf("Archive")).toEqual(["Archive", "F7 remapped", "Issue list"]);
  expect(rowOf("Local Mark")).toEqual(["Local Mark", "No key remapped", "Anywhere"]);
  expect(rowOf("Reset Layout")).toEqual(["Reset Layout", "Ctrl+K L remapped", "Anywhere"]);
  expect(rowOf("Next Change")).toEqual(["Next Change", "No key F7 went to Archive", "Diff"]);
  expect(rowOf("Mute")).toEqual(["Mute", "M", "Issue list"]);

  const tips = screen.getAllByText("remapped").map((mark) => mark.getAttribute("title"));
  expect(tips).toEqual([
    "Set in settings.toml's [keys]. Default: Ctrl+K M",
    "Set in settings.toml's [keys]. Default: no key",
    "Set in settings.toml's [keys]. Default: E",
  ]);
});
