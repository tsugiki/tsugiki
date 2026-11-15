import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { type KeyedRow, remapKeys } from "$lib/state/keymap.svelte";
import KeyList, { type View } from "./KeyList.svelte";

beforeEach(() => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
});

afterEach(() => {
  remapKeys({});
  vi.restoreAllMocks();
});

const show = (views: readonly View[], works: (row: KeyedRow) => boolean = () => true) => {
  const onclose = vi.fn();
  render(KeyList, { here: { views, works }, onclose });
  return onclose;
};

/** The sections of the list, by name. */
const sections = () =>
  within(screen.getByRole("dialog", { name: "Keys" }))
    .getAllByRole("region")
    .map((section) => section.getAttribute("aria-label"));

/** The keys a section of the list names, each with its action's title. */
const keysOf = (section: string) => {
  const terms = within(screen.getByRole("region", { name: section })).getAllByRole("term");
  return terms.map((term) => `${term.textContent} ${term.nextElementSibling?.textContent}`);
};

it("lists the diff's keys first, then those that work anywhere, each by its title", () => {
  show(["diff"]);

  expect(sections()).toEqual(["Diff", "Anywhere"]);
  expect(keysOf("Diff")).toEqual(expect.arrayContaining(["F7 Next Change", "F8 Next Comment Thread"]));
  expect(keysOf("Diff")).not.toContain("E Archive");
  expect(keysOf("Anywhere")).toEqual(
    expect.arrayContaining([
      "Ctrl+Shift+P Command Palette",
      "Ctrl+K Z Show Only the Detail Pane, or the Panes",
    ]),
  );
});

it("lists the Issue list's keys, and leaves out an action with no key and the composer's", () => {
  show(["issue_list"]);
  const all = screen.getAllByRole("term").map((term) => term.nextElementSibling?.textContent);

  expect(keysOf("Issue list")).toEqual([
    "→ Unfold an Issue's PRs",
    "← Fold an Issue's PRs",
    "E Archive",
    "M Mute",
  ]);
  expect(all).not.toContain("Reset Layout");
  expect(all).not.toContain("Send Comment");
});

it("leaves out each key that doesn't work there, and a view with none", () => {
  show(["issue_list", "diff"], (row) => row.context !== "issue_list" && row.action !== "next_change");

  expect(sections()).toEqual(["Diff", "Anywhere"]);
  expect(keysOf("Diff")).not.toContain("F7 Next Change");
});

it("shows a remapped key by its new binding, and an action it took the key from not at all", () => {
  remapKeys({ archive: "a", next_change: "ctrl+k n", mute: "f8" });
  show(["issue_list", "diff"]);

  expect(keysOf("Issue list")).toEqual(expect.arrayContaining(["A Archive", "F8 Mute"]));
  expect(keysOf("Diff")).toContain("Ctrl+K N Next Change");
  // Mute took F8 from Next Comment Thread, which has no key until #550 settles the clash.
  expect(keysOf("Diff").filter((key) => key.endsWith("Next Comment Thread"))).toEqual([]);
});

it("shows a key two remapped actions share only by the one it runs", () => {
  remapKeys({ archive: "x", mute: "x" });
  show(["issue_list"]);
  expect(keysOf("Issue list").filter((key) => key.startsWith("X "))).toEqual(["X Mute"]);
});

it("closes on Esc, Close and a press outside", async () => {
  const onclose = show([]);
  await fireEvent.keyDown(window, { key: "Escape" });
  await fireEvent.keyDown(window, { key: "Enter" });
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await fireEvent.mouseDown(screen.getByTestId("key-list-scrim"));
  // A press inside stays there.
  await fireEvent.mouseDown(screen.getByRole("dialog", { name: "Keys" }));

  expect(onclose).toHaveBeenCalledTimes(3);
});
