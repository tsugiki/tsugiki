import { describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./settings-dialog.svelte";
import { SettingsState } from "./settings.svelte";
import { type DarkQuery, ThemeState } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { rereadSettings: vi.fn(), applySettings: vi.fn() } }));

const LIGHT_SIDE = { matches: false, addEventListener: () => {} } as unknown as DarkQuery;

describe("SettingsDialog", () => {
  it("changes nothing before the core has said what the settings are", async () => {
    const settings = new SettingsState();
    const theme = new ThemeState(LIGHT_SIDE);
    const dialog = new SettingsDialog(settings, theme);
    const edit = vi.fn();
    dialog.change(edit);
    dialog.changeAppearance(edit);
    expect(edit).not.toHaveBeenCalled();
    expect(dialog.held("theme")).toBeUndefined();
    expect(dialog.held("dark")).toBeUndefined();
    expect(dialog.changed).toBe(false);
    expect(dialog.repositories).toEqual([]);
    expect(await dialog.apply()).toBe(true);
    expect(dialog.cancel()).toBe(true);
    expect(settings.settings).toBeUndefined();
  });
});
