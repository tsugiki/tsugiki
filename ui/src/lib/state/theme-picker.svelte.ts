// The theme picker on `Ctrl+K T` (#518), as VS Code's `Ctrl+K Ctrl+T`: Follow system, then every theme of
// both sides, each under its side's heading, previewed on the whole window as the list moves. A pick saves
// the theme in force, `[appearance] theme`, at once (#756). Closing without one puts the theme in force back.
// The user's theme files follow the built-in themes of their side, each file that failed comes greyed with
// why after both sides, and Reload themes reads the folder again (#519).
import type { ThemeKind, ThemeState, ThemeView } from "./theme.svelte";

/** A row of the picker: Follow system, a theme under its side's heading, a theme file that can't be used, or
 * Reload themes. */
export type PickerRow =
  | { kind: "system"; title: string; chosen: boolean }
  | { kind: "theme"; title: string; theme: ThemeView; heading: string; chosen: boolean }
  | { kind: "failed"; title: string; reason: string }
  | { kind: "reload"; title: string };

const HEADINGS: Readonly<Record<ThemeKind, string>> = { light: "Light", dark: "Dark" };
const SIDES: readonly ThemeKind[] = ["light", "dark"];

export class ThemePicker {
  readonly #theme: ThemeState;
  /** Follow system, the themes of each side under its heading, the files of both sides that failed, then
   * Reload themes. The row in force is marked, and the list opens on it. A failed file's row names its file,
   * as a theme the list holds may have its name. */
  rows: PickerRow[] = $derived([
    this.#systemRow(),
    ...SIDES.flatMap((side) => this.#sideRows(side)),
    ...SIDES.flatMap((side) => this.#failedRows(side)),
    { kind: "reload", title: "Reload themes" },
  ]);

  constructor(theme: ThemeState) {
    this.#theme = theme;
  }

  #systemRow(): PickerRow {
    return { kind: "system", title: "Follow system", chosen: this.#theme.themes?.theme.kind === "system" };
  }

  /** Side `side`'s themes, under its heading. One a theme the settings keep, by its name or a mode from
   * before, is in force. */
  #sideRows(side: ThemeKind): PickerRow[] {
    const kept = this.#theme.themes?.theme.kind !== "system";
    const inForce = kept ? this.#theme.current?.name : undefined;
    return this.#theme.themesOf(side).map((theme) => ({
      kind: "theme",
      title: theme.name,
      theme,
      heading: HEADINGS[side],
      chosen: theme.name === inForce,
    }));
  }

  #failedRows(side: ThemeKind): PickerRow[] {
    return this.#theme
      .failedOf(side)
      .map(({ name, file, reason }) => ({ kind: "failed", title: `${name} (${file})`, reason }));
  }

  /** The list has moved to `row`: a theme shows on the whole window, with no fade, and Follow system shows
   * the theme of the side Windows is on. Every other row shows the theme in force, as it chooses nothing. */
  move(row: PickerRow): void {
    this.#theme.preview =
      row.kind === "theme" ? row.theme : row.kind === "system" ? this.#theme.following : undefined;
  }

  /** Saves `row`'s theme, or following the system, as the theme in force. */
  pick(row: PickerRow & { kind: "system" | "theme" }): Promise<void> {
    this.#theme.preview = undefined;
    return this.#theme.pick(row.kind === "system" ? "system" : row.theme);
  }

  /** Reads the themes folder again, so a file edited since shows in the list without a restart. */
  reload(): Promise<void> {
    return this.#theme.load();
  }

  /** Closes without a pick, and puts the theme in force back. */
  cancel(): void {
    this.#theme.preview = undefined;
  }
}
