<!-- PROTOTYPE, throwaway (#367): lays the variant out around the Files tab's diff. A: a bar above it and a
     Submit dialog. B: a panel below it. C: a rail beside it. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Composer from "./Composer.svelte";
  import DraftCard from "./DraftCard.svelte";
  import { proto } from "./pending-review.svelte";
  import ReviewStatus from "./ReviewStatus.svelte";
  import SubmitForm from "./SubmitForm.svelte";
  import ReviewSwitcher from "./ReviewSwitcher.svelte";

  let { children }: { children: Snippet } = $props();

  const count = $derived(proto.commentCount);
  /** The composer B and C show outside the diff: every one in B and C, a conversation one never. */
  const outside = $derived(proto.composer && proto.composer.target.kind !== "conversation" ? proto.composer : undefined);
  let panelHeight = $state(300);

  function drag(e: PointerEvent) {
    const startY = e.clientY;
    const start = panelHeight;
    const move = (m: PointerEvent) => (panelHeight = Math.max(140, start + startY - m.clientY));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape" && proto.submitOpen) proto.submitOpen = false;
  }}
/>

{#if proto.variant === "A"}
  <div class="host column">
    <div class="bar" class:has={count > 0}>
      <span class="pending">
        {#if count > 0}✎ Pending review · <b>{count}</b> {count === 1 ? "draft" : "drafts"}{:else}No Pending review{/if}
      </span>
      <button
        type="button"
        class="primary"
        onclick={() => (proto.submitOpen = true)}
        disabled={proto.readOnly}
        title={proto.readOnly ? "Read-only: your token can't write to this Repository. Submit the review on GitHub." : ""}
      >
        Submit review…
      </button>
      <span class="grow"></span>
      <ReviewStatus />
    </div>
    {@render children()}
  </div>
  {#if proto.submitOpen}
    <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (proto.submitOpen = false)}>
      <div class="dialog" role="dialog" aria-modal="true" aria-label="Submit your review">
        <SubmitForm layout="dialog" />
      </div>
    </div>
  {/if}
{:else if proto.variant === "B"}
  <div class="host column">
    {@render children()}
    <div class="panel" style:height={`${String(panelHeight)}px`}>
      <div class="edge" role="separator" aria-orientation="horizontal" onpointerdown={drag}></div>
      <div class="ptabs">
        <button type="button" class:on={proto.panelTab === "comment"} onclick={() => (proto.panelTab = "comment")}>
          Comment{outside ? " ●" : ""}
        </button>
        <button type="button" class:on={proto.panelTab === "review"} onclick={() => (proto.panelTab = "review")}>
          Pending review{count > 0 ? ` (${String(count)})` : ""}
        </button>
        <span class="grow"></span>
        <ReviewStatus />
      </div>
      <div class="pbody">
        {#if proto.panelTab === "comment"}
          {#if outside}
            <Composer composer={outside} layout="panel" />
          {:else}
            <p class="empty">Click a line number to comment on it; Shift+click or drag for a range. Reply from a thread.</p>
          {/if}
        {:else}
          <div class="review">
            <div class="list">
              {#each proto.drafts as draft (draft.id)}
                <DraftCard {draft} mode="list" />
              {:else}
                <p class="empty">No drafts yet.</p>
              {/each}
            </div>
            <div class="form"><SubmitForm layout="panel" /></div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{:else}
  <div class="host row">
    {@render children()}
    <aside class="rail" aria-label="Your review">
      <div class="rstatus"><ReviewStatus /></div>
      <div class="rhead">
        {#if count > 0}✎ Pending review · <b>{count}</b> {count === 1 ? "draft" : "drafts"}{:else}No Pending review yet{/if}
      </div>
      {#if outside}
        <div class="rcomposer"><Composer composer={outside} layout="rail" /></div>
      {/if}
      <div class="rlist">
        {#each proto.drafts as draft (draft.id)}
          <DraftCard {draft} mode="list" />
        {:else}
          {#if !outside}<p class="empty">Click a line number to comment; Shift+click or drag for a range.</p>{/if}
        {/each}
      </div>
      <div class="rform"><SubmitForm layout="rail" /></div>
    </aside>
  </div>
{/if}

<ReviewSwitcher />

<style>
  .host {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .column {
    flex-direction: column;
  }

  .row {
    flex-direction: row;
  }

  .row > :global(.diff) {
    min-width: 0;
  }

  .bar {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 4px 8px;
    font: 12px system-ui, sans-serif;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .bar.has {
    background: color-mix(in srgb, #8250df 8%, transparent);
  }

  .grow {
    flex: 1;
  }

  .primary {
    padding: 2px 10px;
    font: inherit;
    color: #fff;
    background: #1f883d;
    border: 1px solid #1a7f37;
    border-radius: 4px;
    cursor: pointer;
  }

  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 30%);
  }

  .dialog {
    width: min(560px, 90vw);
    padding: 14px 16px;
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 10px 40px rgb(0 0 0 / 35%);
  }

  .panel {
    position: relative;
    display: flex;
    flex: none;
    flex-direction: column;
    border-top: 2px solid #0969da;
  }

  .edge {
    position: absolute;
    top: -4px;
    left: 0;
    right: 0;
    height: 6px;
    cursor: ns-resize;
  }

  .ptabs {
    display: flex;
    gap: 4px;
    align-items: center;
    padding: 2px 8px;
    font: 12px system-ui, sans-serif;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .ptabs > button {
    all: unset;
    padding: 4px 8px;
    cursor: pointer;
  }

  .ptabs > button.on {
    border-bottom: 2px solid currentColor;
  }

  .pbody {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .review {
    display: flex;
    gap: 12px;
    height: 100%;
  }

  .review .list {
    flex: 1;
    overflow: auto;
  }

  .review .form {
    flex: 1;
    padding: 6px 8px;
  }

  .empty {
    padding: 8px;
    margin: 0;
    font: 12px system-ui, sans-serif;
    opacity: 0.6;
  }

  .rail {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 340px;
    border-left: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    font: 12px system-ui, sans-serif;
  }

  .rstatus,
  .rhead {
    padding: 4px 8px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }

  .rcomposer {
    border-bottom: 2px solid #0969da;
  }

  .rlist {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .rform {
    padding: 8px;
    border-top: 1px solid color-mix(in srgb, currentColor 18%, transparent);
  }
</style>
