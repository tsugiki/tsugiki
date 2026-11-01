<script lang="ts">
  // PROTOTYPE — throwaway. Question: what should Settings look like? Three variants inside the app,
  // switchable with ?variant=A|B|C and the floating bar. All edit the same in-memory settings.
  import { settingsUi } from "$lib/state/settings.svelte";
  import SettingsA from "./SettingsA.svelte";
  import SettingsB from "./SettingsB.svelte";
  import SettingsC from "./SettingsC.svelte";
  import VariantBar from "./VariantBar.svelte";

  const VARIANTS: [string, string][] = [["A", "Searchable list"], ["B", "Dialog"], ["C", "The files"]];
  const close = () => (settingsUi.open = false);
  // B handles Esc itself (it means Cancel there).
  function esc(e: KeyboardEvent) { if (e.key === "Escape" && !e.defaultPrevented && settingsUi.variant !== "B") { e.preventDefault(); close(); } }
</script>

<svelte:window onkeydown={esc} />

{#key settingsUi.variant}
  {#if settingsUi.variant === "B"}<SettingsB onClose={close} />
  {:else if settingsUi.variant === "C"}<SettingsC onClose={close} />
  {:else}<SettingsA onClose={close} />{/if}
{/key}
<VariantBar variants={VARIANTS} current={settingsUi.variant} onChange={(v) => (settingsUi.variant = v)} />
