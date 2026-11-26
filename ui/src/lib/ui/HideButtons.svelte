<!--
  The Archive, Mute and Mute author buttons of the Issue view and the PR view of a Standalone PR (#490, #492,
  #494): each acts on the item on show, and turns into its undo once used. A Muted item offers only Unmute and
  Mute author, a Muted author's only Unmute author, and Mute author is off on the user's own item.
-->
<script lang="ts" generics="T extends { hidden: Hidden | null; mute_author: MuteAuthor }">
  import { type Hidden, isMuted, type MuteAuthor } from "$lib/state/workbench.svelte";
  import { muteAuthorWords } from "./workbench-row";

  let {
    view,
    onarchive,
    onmute,
    onmuteauthor,
  }: {
    view: T;
    onarchive: (view: T) => void;
    onmute: (view: T) => void;
    onmuteauthor: (view: T) => void;
  } = $props();

  const muted = $derived(isMuted(view.hidden));
</script>

{#if !muted}
  <button onclick={() => onarchive(view)}>{view.hidden === "archived" ? "Unarchive" : "Archive"}</button>
{/if}
{#if view.hidden !== "muted_author"}
  <button onclick={() => onmute(view)}>{muted ? "Unmute" : "Mute"}</button>
{/if}
<button disabled={view.mute_author.kind === "off"} onclick={() => onmuteauthor(view)}>
  {muteAuthorWords(view.mute_author)}
</button>
