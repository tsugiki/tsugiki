import { describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./settings-dialog.svelte";
import { SettingsState } from "./settings.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { rereadSettings: vi.fn(), applySettings: vi.fn() } }));

describe("SettingsDialog", () => {
  it("changes nothing before the core has said what the settings are", async () => {
    const settings = new SettingsState();
    const dialog = new SettingsDialog(settings);
    const edit = vi.fn();
    dialog.change(edit);
    expect(edit).not.toHaveBeenCalled();
    expect(dialog.changed).toBe(false);
    expect(dialog.repositories).toEqual([]);
    expect(await dialog.apply()).toBe(true);
    expect(dialog.cancel()).toBe(true);
    expect(settings.settings).toBeUndefined();
  });
});
