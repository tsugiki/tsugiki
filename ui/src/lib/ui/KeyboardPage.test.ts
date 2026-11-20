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

/** The table's rows below its header, each as its cells' words, one space apart. */
const rows = () =>
  within(screen.getByRole("table"))
    .getAllByRole("row")
    .slice(1)
    .map((row) =>
      within(row)
        .getAllByRole("cell")
        .map((cell) => cell.textContent.trim().split(/\s+/).join(" ")),
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
  expect(rowOf("Unfold an Issue's PRs")).toEqual(["Unfold an Issue's PRs", "→", "Issue list"]);
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
  expect(rows()).toEqual([["Archive", "E", "Anywhere"]]);

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

it("finds the actions with no key by the words the page shows", async () => {
  render(KeyboardPage);
  await filter("no key");
  expect(rows()).toEqual([
    ["Reset Layout", "No key", "Anywhere"],
    ["Mute Author", "No key", "Anywhere"],
  ]);
});

it("says so when no row holds what the box holds", async () => {
  render(KeyboardPage);
  await filter("no such action");
  expect(rows()).toEqual([["No action or key holds that."]]);
});

it("clears the box on Esc, and lets Esc through to the dialog once the box is empty", async () => {
  const heard = vi.fn();
  window.addEventListener("keydown", heard);
  render(KeyboardPage);
  const box = screen.getByRole("searchbox", { name: "Filter keys" });
  await filter("archive");

  await fireEvent.keyDown(box, { key: "Escape" });
  expect([box, heard.mock.calls.length]).toEqual([expect.objectContaining({ value: "" }), 0]);
  expect(rows()).toHaveLength(keyTable().length);

  await fireEvent.keyDown(box, { key: "a" });
  await fireEvent.keyDown(box, { key: "Escape" });
  expect(heard).toHaveBeenCalledTimes(2);
  // A box of spaces filters nothing, so Esc goes through.
  await filter("   ");
  await fireEvent.keyDown(box, { key: "Escape" });
  expect(heard).toHaveBeenCalledTimes(3);
  window.removeEventListener("keydown", heard);
});

it("leaves the box to an input method whose candidates Esc cancels", async () => {
  const heard = vi.fn();
  window.addEventListener("keydown", heard);
  render(KeyboardPage);
  const box = screen.getByRole("searchbox", { name: "Filter keys" });
  await filter("archive");

  await fireEvent.keyDown(box, { key: "Escape", isComposing: true });
  expect([box, heard.mock.calls.length]).toEqual([expect.objectContaining({ value: "archive" }), 1]);
  window.removeEventListener("keydown", heard);
});

it("marks a remapped key, with its default, and says which action took a key", () => {
  remapKeys({ archive: "f7", mute: "f8", local_mark: "", reset_layout: "ctrl+k l", pin: "ctrl+k p" });
  render(KeyboardPage);

  expect(rowOf("Mute")).toEqual(["Mute", "F8 remapped", "Anywhere"]);
  expect(rowOf("Local Mark")).toEqual(["Local Mark", "No key remapped", "Anywhere"]);
  expect(rowOf("Reset Layout")).toEqual(["Reset Layout", "Ctrl+K L remapped", "Anywhere"]);
  expect(rowOf("Next Change")).toEqual(["Next Change", "No key F7 went to Archive", "Diff"]);
  expect(rowOf("Next Comment Thread")).toEqual(["Next Comment Thread", "No key F8 went to Mute", "Diff"]);
  // A line that gives an action its own default isn't marked.
  expect(rowOf("Pin or Unpin")?.[1]).toBe("Ctrl+K P");

  const tips = screen.getAllByText("remapped").map((mark) => mark.getAttribute("title"));
  expect(tips).toEqual([
    "Set in settings.toml's [keys]. Default: Ctrl+K M",
    "Set in settings.toml's [keys]. Default: No key",
    "Set in settings.toml's [keys]. Default: E",
    "Set in settings.toml's [keys]. Default: M",
  ]);
});

it("marks a remapped key a later remapped action took as lost to it", () => {
  remapKeys({ archive: "f7", mute: "f7" });
  render(KeyboardPage);

  expect(rowOf("Archive")).toEqual(["Archive", "No key remapped F7 went to Mute", "Anywhere"]);
  expect(rowOf("Mute")).toEqual(["Mute", "F7 remapped", "Anywhere"]);
  expect(rowOf("Next Change")).toEqual(["Next Change", "No key F7 went to Mute", "Diff"]);
});

it("finds a row by the key another action took from it, and by that action", async () => {
  remapKeys({ archive: "f7" });
  render(KeyboardPage);

  await filter("f7");
  expect(rows().map(([action]) => action)).toEqual(["Next Change", "Previous Change", "Archive"]);
  await filter("went to archive");
  expect(rows().map(([action]) => action)).toEqual(["Next Change"]);
});
