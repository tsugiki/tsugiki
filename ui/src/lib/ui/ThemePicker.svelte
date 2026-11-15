<script lang="ts">
  // The theme picker on `Ctrl+K T` (#518): the command palette's quick pick over Follow system and every
  // theme, under the heading of its side (#756). `ThemePicker` in `theme-picker.svelte.ts` decides what each
  // row does.
  import { untrack } from "svelte";
  import { type PickerRow, ThemePicker } from "$lib/state/theme-picker.svelte";
  import type { ThemeState } from "$lib/state/theme.svelte";
  import Palette, { type PaletteCommand, type PaletteWords } from "./Palette.svelte";

  let {
    theme,
    onclose,
    onsaved,
  }: {
    theme: ThemeState;
    onclose: () => void;
    /** Runs once a pick's save has ended, whether it worked or not, and the core has read the files again. */
    onsaved: () => void;
  } = $props();

  const picker = new ThemePicker(untrack(() => theme));
  const WORDS: PaletteWords = {
    dialog: "Theme picker",
    input: "Theme",
    placeholder: "Type a theme's name",
    list: "Themes",
    none: "No theme matches",
    mark: "chosen",
  };

  /** Each row previews while it's picked. A file that failed shows greyed with why, and Reload themes acts
   * without closing the list. A theme lists under its side's heading. */
  const command = (row: PickerRow): PaletteCommand => {
    const preview = () => {
      picker.move(row);
    };
    switch (row.kind) {
      case "system":
      case "theme":
        return {
          title: row.title,
          current: row.chosen,
          group: row.kind === "theme" ? row.heading : undefined,
          preview,
          run: () => void picker.pick(row).then(onsaved),
        };
      case "failed":
        return { title: row.title, note: row.reason, disabled: true, preview };
      case "reload":
        return { title: row.title, keepsOpen: true, preview, run: () => void picker.reload() };
    }
  };

  const commands: PaletteCommand[] = $derived(picker.rows.map(command));
  /** The row the list opens on: the one in force. */
  const start = $derived(
    Math.max(
      0,
      commands.findIndex((one) => one.current),
    ),
  );
</script>

<Palette
  {commands}
  words={WORDS}
  {start}
  oncancel={() => {
    picker.cancel();
  }}
  {onclose}
/>
