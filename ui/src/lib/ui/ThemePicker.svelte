<script lang="ts">
  // The theme picker on `Ctrl+K T` (#518): the command palette's quick pick over the themes of one side.
  // `ThemePicker` in `theme-picker.svelte.ts` decides what each row does.
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
  const words: PaletteWords = $derived({
    dialog: "Theme picker",
    input: "Theme",
    placeholder: "Type a theme's name",
    list: picker.side === "light" ? "Light themes" : "Dark themes",
    none: "No theme matches",
    mark: "chosen",
  });

  /** Each row previews while it's picked, and the last row switches the list without closing it. */
  const command = (row: PickerRow): PaletteCommand => {
    const preview = () => {
      picker.move(row);
    };
    return row.kind === "theme"
      ? { title: row.title, current: row.chosen, preview, run: () => void picker.pick(row).then(onsaved) }
      : {
          title: row.title,
          keepsOpen: true,
          preview,
          run: () => {
            picker.switchSide();
          },
        };
  };
</script>

<!-- A new list for each side, so it opens on that side's choice with nothing typed. -->
{#key picker.side}
  <Palette
    commands={picker.rows.map(command)}
    {words}
    start={picker.start}
    oncancel={() => {
      picker.cancel();
    }}
    {onclose}
  />
{/key}
