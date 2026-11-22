import { describe, expect, it, vi } from "vitest";
import { defaultSettings, NO_SERVERS, ok } from "$lib/testing/fixtures";
import type { Home } from "./home.svelte";
import { SettingsDialog } from "./settings-dialog.svelte";
import { SettingsState } from "./settings.svelte";
import { type DarkQuery, ThemeState } from "./theme.svelte";

const core = vi.hoisted(() => ({
  rereadSettings: vi.fn(),
  applySettings: vi.fn(),
  onThisMachine: vi.fn(),
  settings: vi.fn(),
}));
vi.mock("$lib/data/bindings", () => ({ commands: core }));

const LIGHT_SIDE = { matches: false, addEventListener: () => {} } as unknown as DarkQuery;

/** What the window opens on once Add repository has added some-maintainer/gcmon. */
const addedGcmon: Home = {
  screen: "repository",
  repository: { name: "some-maintainer/gcmon", conventions: "github", filters: [] },
  others: ["python/cpython"],
};

describe("SettingsDialog", () => {
  it("changes nothing before the core has said what the settings are", async () => {
    core.onThisMachine.mockResolvedValue(ok([]));
    core.settings.mockReturnValue(new Promise(() => {}));
    const settings = new SettingsState();
    const theme = new ThemeState(LIGHT_SIDE);
    const dialog = new SettingsDialog(settings, theme);
    const edit = vi.fn();
    dialog.change(edit);
    dialog.changeAppearance(edit);
    dialog.added(addedGcmon);
    expect(edit).not.toHaveBeenCalled();
    expect(dialog.held("theme")).toBeUndefined();
    expect(dialog.held("dark")).toBeUndefined();
    expect(dialog.changed).toBe(false);
    expect(dialog.repositories).toEqual([]);
    expect(await dialog.apply()).toBe(true);
    expect(dialog.cancel()).toBe(true);
    expect(settings.settings).toBeUndefined();
  });

  it("lists a Repository Add repository added once, Cancel keeps it, and the settings load as it closes", async () => {
    core.onThisMachine.mockResolvedValue(ok([]));
    const cpython = { name: "python/cpython", conventions: "cpython" as const, filters: [] };
    const files = { settings: defaultSettings({ repositories: [cpython] }), language_servers: NO_SERVERS };
    core.rereadSettings.mockResolvedValue(ok({ ...files, problems: [] }));
    core.settings.mockReturnValue(new Promise(() => {}));
    const settings = new SettingsState();
    const dialog = new SettingsDialog(settings, new ThemeState(LIGHT_SIDE));
    await dialog.open();

    dialog.added(addedGcmon);
    dialog.added(addedGcmon);
    dialog.added({ screen: "onboarding", token: "gh" });

    expect(dialog.repositories).toEqual(["python/cpython", "some-maintainer/gcmon"]);
    expect(dialog.changed).toBe(false);
    await vi.waitFor(() => {
      expect(core.onThisMachine).toHaveBeenCalledTimes(3);
    });
    expect(core.settings).not.toHaveBeenCalled();
    dialog.cancel();
    expect(settings.settings?.repositories.map((repository) => repository.name)).toEqual([
      "python/cpython",
      "some-maintainer/gcmon",
    ]);
    expect(core.settings).toHaveBeenCalledOnce();
  });

  it("takes what the newest ask measured, though an older answer comes after it", async () => {
    const older = Promise.withResolvers<unknown>();
    const measured = (mb: number) => [
      { repository: "python/cpython", source: null, mirror: { kind: "measured", mb } },
    ];
    core.onThisMachine.mockReturnValueOnce(older.promise).mockResolvedValueOnce(ok(measured(2)));
    const dialog = new SettingsDialog(new SettingsState(), new ThemeState(LIGHT_SIDE));

    const first = dialog.measure();
    await dialog.measure();
    older.resolve(ok(measured(1)));
    await first;

    expect(dialog.onThisMachine("python/cpython")?.mirror).toEqual({ kind: "measured", mb: 2 });
  });
});
