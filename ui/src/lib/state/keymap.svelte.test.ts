import { afterEach, describe, expect, it } from "vitest";
import {
  actionOn,
  chordFirst,
  isChordStart,
  keyOf,
  keysIn,
  keyTable,
  ownsKey,
  remapKeys,
  startsChordOf,
} from "./keymap.svelte";

afterEach(() => {
  remapKeys({});
});

describe("the key table", () => {
  it("finds the action on a key or a chord", () => {
    expect([actionOn("ctrl+shift+p"), actionOn("ctrl+k m"), actionOn("ctrl+c"), actionOn(null)]).toEqual([
      "command_palette",
      "local_mark",
      undefined,
      undefined,
    ]);
  });

  it("finds an action's key", () => {
    expect([keyOf("archive"), keyOf("toggle_detail_only"), keyOf("reset_layout")]).toEqual([
      "e",
      "ctrl+k z",
      null,
    ]);
  });

  it("moves a remapped action to its new key, and leaves its old key with none", () => {
    remapKeys({ archive: "a" });
    expect([actionOn("a"), actionOn("e"), keyOf("archive")]).toEqual(["archive", undefined, "a"]);
    expect(keyTable().find((row) => row.action === "archive")?.key).toBe("a");
  });

  it("leaves an action remapped to nothing with no key", () => {
    remapKeys({ local_mark: "" });
    expect([actionOn("ctrl+k m"), keyOf("local_mark")]).toEqual([undefined, null]);
  });

  it("lets a remapped action keep its key over the default it lands on, which shows no key", () => {
    remapKeys({ quick_open: "e" });
    expect([actionOn("e"), actionOn("ctrl+p"), keyOf("archive")]).toEqual(["quick_open", undefined, null]);
    expect(keyTable().find((row) => row.action === "archive")?.key).toBeNull();
  });

  it("leaves the bare ← and → alone whichever action has them, and owns Fold's remapped key", () => {
    remapKeys({ unfold: "ctrl+shift+arrowright" });
    expect([ownsKey("ctrl+shift+arrowright"), ownsKey("arrowleft"), ownsKey("ctrl+shift+i")]).toEqual([
      true,
      false,
      false,
    ]);
  });

  it("knows the chords' first presses and the keys the guard owns, with the remaps", () => {
    expect([isChordStart("ctrl+k"), isChordStart("ctrl+j"), ownsKey("e"), ownsKey("arrowright")]).toEqual([
      true,
      false,
      true,
      false,
    ]);
    remapKeys({ pin: "ctrl+j p", archive: "" });
    expect([isChordStart("ctrl+j"), ownsKey("ctrl+j"), ownsKey("e")]).toEqual([true, true, false]);
  });

  it("knows whether a press starts the chord of one of some actions", () => {
    remapKeys({ open_settings: "ctrl+j s" });
    expect([
      startsChordOf("ctrl+j", ["open_settings", "pin"]),
      startsChordOf("ctrl+k", ["open_settings"]),
      startsChordOf("ctrl+k", ["pin"]),
    ]).toEqual([true, false, true]);
  });

  it("starts no chord on an action with no key or a key of one press", () => {
    expect([startsChordOf("ctrl+k", ["reset_layout"]), startsChordOf("ctrl+shift", ["toggle_hud"])]).toEqual([
      false,
      false,
    ]);
  });

  it("names a chord's first press", () => {
    expect(chordFirst("ctrl+k z")).toBe("ctrl+k");
  });

  it("lists in a context only the keys that run their action", () => {
    remapKeys({ archive: "x", mute: "x" });
    const listed = keysIn("issue_list").map((row) => row.action);
    expect(listed).toContain("mute");
    expect(listed).not.toContain("archive");
  });

  it("puts each default back once the remaps change again", () => {
    remapKeys({ archive: "a", reset_layout: "ctrl+k l" });
    remapKeys({});
    expect([actionOn("e"), actionOn("a"), keyOf("reset_layout")]).toEqual(["archive", undefined, null]);
  });
});
