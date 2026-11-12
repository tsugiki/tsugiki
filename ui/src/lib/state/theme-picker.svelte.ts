// The theme picker on `Ctrl+K T` (#518), as VS Code's `Ctrl+K Ctrl+T`: the themes of one side, previewed on
// the whole window as the list moves, then a row that switches the list to the other side. A pick saves the
// side's choice at once. Closing without one puts the theme in force back.
import type { ThemeKind, ThemeState, ThemeView } from "./theme.svelte";

/** A row of the picker: a theme of the side listed, or the last row, which lists the other side. */
export type PickerRow =
  { kind: "theme"; title: string; theme: ThemeView; chosen: boolean } | { kind: "switch"; title: string };

const OTHER: Readonly<Record<ThemeKind, ThemeKind>> = { light: "dark", dark: "light" };
const SWITCH_TITLES: Readonly<Record<ThemeKind, string>> = { light: "Light themes…", dark: "Dark themes…" };

export class ThemePicker {
  readonly #theme: ThemeState;
  /** The side whose themes the list shows: the side in use when the picker opens. */
  side = $state<ThemeKind>("light");
  /** The themes of `side`, the one chosen for it marked, then the row for the other side. */
  rows: PickerRow[] = $derived([
    ...this.#themeRows(),
    { kind: "switch", title: SWITCH_TITLES[OTHER[this.side]] },
  ]);
  /** The row the list opens on: the side's chosen theme. */
  start = $derived(
    Math.max(
      0,
      this.rows.findIndex((row) => row.kind === "theme" && row.chosen),
    ),
  );

  constructor(theme: ThemeState) {
    this.#theme = theme;
    this.side = theme.side;
  }

  #themeRows(): PickerRow[] {
    const chosen = this.#theme.chosen(this.side);
    return this.#theme
      .themesOf(this.side)
      .map((theme) => ({ kind: "theme", title: theme.name, theme, chosen: theme.name === chosen }));
  }

  /** The list has moved to `row`: a theme shows on the whole window, with no fade. The last row shows the
   * theme in force, as it chooses nothing. */
  move(row: PickerRow): void {
    this.#theme.preview = row.kind === "theme" ? row.theme : undefined;
  }

  /** Chooses `row`'s theme for the side listed and saves it. The window then shows the theme in force,
   * which is this one only if its side is in use: a pick from the other side leaves the window alone. */
  pick(row: PickerRow & { kind: "theme" }): Promise<void> {
    this.#theme.preview = undefined;
    return this.#theme.choose(this.side, row.theme.name);
  }

  /** Lists the other side's themes. */
  switchSide(): void {
    this.side = OTHER[this.side];
  }

  /** Closes without a pick, and puts the theme in force back. */
  cancel(): void {
    this.#theme.preview = undefined;
  }
}
