// The settings the core read when it started. The core owns them, so this only asks once (ADR 0014).
import { commands, type DiffFiles } from "$lib/data/bindings";

export class SettingsState {
  // Off until the core answers, so a status bar the settings hide doesn't appear for a moment.
  statusBarVisible = $state(false);
  problems = $state<string[]>([]);
  /** Whether each PR's Files tab opens on all its files or one at a time (#605). */
  diffFiles = $state<DiffFiles>("all");

  async load() {
    try {
      const { settings, problems } = await commands.settings();
      this.statusBarVisible = settings.status_bar.visible;
      this.diffFiles = settings.diff.files;
      this.problems = problems;
    } catch (e) {
      this.statusBarVisible = true;
      this.problems = [`Tsugiki can't read its settings: ${String(e)}`];
    }
  }
}
