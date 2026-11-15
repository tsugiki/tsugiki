import { describe, expect, it } from "vitest";
import { actionOn, keyOf } from "./keymap";

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
    expect([keyOf("archive"), keyOf("toggle_detail_only")]).toEqual(["e", "ctrl+k z"]);
  });
});
