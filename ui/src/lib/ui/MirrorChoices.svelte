<!-- The two ways a Repository's mirror can get its objects, with the time each takes and the disk the mirror
     takes either way, and the clone the user picked to borrow from (#172, #311). -->
<script lang="ts">
  import type { MirrorChoosing } from "$lib/state/mirror-choice.svelte";
  import type { MirrorCost } from "$lib/state/mirror-source";
  import { pickedWords } from "./clone-words";
  import { describeMirror } from "./repository-check";

  let {
    repository,
    cost,
    choosing,
    disabled = false,
  }: {
    repository: string;
    /** What the mirror takes, or nothing while GitHub hasn't said. */
    cost: MirrorCost | null;
    choosing: MirrorChoosing;
    disabled?: boolean;
  } = $props();

  // Two of these can be in the page at once, the Files tab's and a dialog's, and each is a radio group.
  const group = $props.id();
  const words = $derived(describeMirror(cost));
  const picked = $derived(choosing.choice.kind === "borrow" ? choosing.choice.picked : undefined);
</script>

<fieldset {disabled}>
  <legend>{words.disk}</legend>
  <label>
    <input
      type="radio"
      name={group}
      checked={choosing.choice.kind === "download"}
      onchange={() => {
        choosing.chooseDownload();
      }}
    />
    <span>{words.download}</span>
  </label>
  <label>
    <input
      type="radio"
      name={group}
      checked={choosing.choice.kind === "borrow"}
      onchange={() => void choosing.chooseClone(repository)}
    />
    <span>{words.borrow}</span>
  </label>
  {#if picked}
    <p class="picked">
      <span>{pickedWords(picked, repository)}</span>
      <button type="button" onclick={() => void choosing.chooseClone(repository)}
        >Choose another clone…</button
      >
    </p>
  {/if}
</fieldset>

<style>
  fieldset {
    margin: 12px 0 0;
    padding: 4px 8px 8px;
    border: 1px solid var(--border);
  }

  label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin-top: 4px;
  }

  p {
    margin: 8px 0 0;
    overflow-wrap: anywhere;
  }

  .picked {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin-left: 24px;
  }
</style>
