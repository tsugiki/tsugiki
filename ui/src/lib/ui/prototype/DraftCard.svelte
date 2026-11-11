<!-- PROTOTYPE, throwaway (#367): a draft in the user's Pending review. `full` under its line in the diff (A),
     `line` as a one-line mark under its line (B and C), `list` as an entry in the panel or the rail. -->
<script lang="ts">
  import { render } from "./markdown";
  import { proto, targetWords, type Draft } from "./pending-review.svelte";

  let { draft, mode }: { draft: Draft; mode: "full" | "line" | "list" } = $props();

  const first = $derived(draft.body.split("\n").find((one) => one.trim() !== "" && !one.startsWith("```")) ?? "");
  const replaced = $derived(
    draft.target.kind === "lines" ? draft.target.lines.filter((one) => one.kind !== "removed").map((one) => one.text) : [],
  );
</script>

{#if mode === "line"}
  <button type="button" class="line" onclick={() => proto.edit(draft)} title="Edit in the {proto.variant === 'B' ? 'panel' : 'rail'}">
    <span class="tag">Pending, yours</span>
    {#if draft.checkLine}<span class="tag warn">check its line</span>{/if}
    <span class="first">{first}</span>
  </button>
{:else}
  <div class={`card ${mode}`}>
    <div class="head">
      {#if mode === "list"}
        <button type="button" class="link where" onclick={() => proto.jumpTo(draft.target)}>{targetWords(draft.target)}</button>
      {:else}
        <b>you</b>
      {/if}
      <span class="tag">Pending{draft.fromGitHub ? " · begun on github.com" : ""}</span>
      {#if draft.checkLine}<span class="tag warn">check its line</span>{/if}
      <span class="grow"></span>
      <button type="button" class="link" disabled={proto.readOnly} onclick={() => proto.edit(draft)}>Edit</button>
      <button type="button" class="link" disabled={proto.readOnly} onclick={() => proto.remove(draft)}>Delete</button>
    </div>
    {#if draft.checkLine}
      <p class="warn small">
        GitHub placed this on a newer push than the diff shows, so its line may have moved. Delete it and place it
        again if it's wrong.
      </p>
    {/if}
    {#if mode === "full"}
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      <div class="md">{@html render(draft.body, replaced)}</div>
    {:else}
      <div class="first">{first}</div>
    {/if}
  </div>
{/if}

<style>
  .line {
    all: unset;
    display: flex;
    gap: 8px;
    align-items: center;
    height: 20px;
    padding-left: 96px;
    font: 12px system-ui, sans-serif;
    cursor: pointer;
    background: color-mix(in srgb, #8250df 8%, transparent);
    width: 100%;
    box-sizing: border-box;
  }

  .line:hover {
    background: color-mix(in srgb, #8250df 16%, transparent);
  }

  .card {
    font: 13px system-ui, sans-serif;
  }

  .card.full {
    margin: 4px 12px 4px 96px;
    padding: 6px 10px;
    border: 1px solid color-mix(in srgb, #8250df 40%, transparent);
    border-left: 3px solid #8250df;
    border-radius: 4px;
    background: color-mix(in srgb, #8250df 5%, Canvas);
  }

  .card.list {
    padding: 6px 8px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }

  .head {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .grow {
    flex: 1;
  }

  .tag {
    padding: 0 6px;
    font-size: 11px;
    border-radius: 999px;
    color: #8250df;
    border: 1px solid color-mix(in srgb, #8250df 50%, transparent);
  }

  .tag.warn,
  .warn {
    color: #9a6700;
    border-color: #d4a72c;
  }

  .small {
    margin: 4px 0;
    font-size: 12px;
  }

  .first {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.85;
  }

  .where {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }

  .link {
    all: unset;
    cursor: pointer;
    font-size: 12px;
    text-decoration: underline;
  }

  .link:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .md :global(p) {
    margin: 4px 0;
  }

  .md :global(.suggestion) {
    font: 12px ui-monospace, Consolas, monospace;
    white-space: pre;
    border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
  }

  .md :global(.shead) {
    padding: 2px 6px;
    font: 12px system-ui, sans-serif;
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  .md :global(.del) {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }

  .md :global(.add) {
    background: color-mix(in srgb, #2da44e 18%, transparent);
  }
</style>
