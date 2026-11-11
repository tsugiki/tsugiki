<!-- PROTOTYPE, throwaway (#367): submitting the Pending review. A dialog in A, a tab of the panel in B, the
     rail's foot in C. -->
<script lang="ts">
  import { EVENTS, proto } from "./pending-review.svelte";

  let { layout }: { layout: "dialog" | "panel" | "rail" } = $props();

  const fromGitHub = $derived(proto.drafts.filter((one) => one.fromGitHub).length);
  const count = $derived(proto.commentCount);
  const busy = $derived(proto.submitState.kind === "sending");
  const ownBlocked = (key: string) => proto.own && key !== "COMMENT";

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      void proto.submit();
    } else if (e.key === "Escape" && layout === "dialog") {
      e.preventDefault();
      proto.submitOpen = false;
    }
  }
</script>

<div class={`submit ${layout}`} role="group" aria-label="Submit your review">
  {#if layout !== "rail"}
    <div class="title">Submit your review</div>
  {/if}
  <div class="count">
    {#if count === 0}
      No drafts yet{proto.pending ? "" : " · no Pending review is open"}
    {:else}
      {count === 1 ? "1 draft" : `${String(count)} drafts`} in your Pending review{fromGitHub ? ` (${String(fromGitHub)} begun on github.com)` : ""}
    {/if}
  </div>

  {#if proto.submitState.kind === "gone"}
    <p class="bad">
      Your Pending review was submitted or discarded on GitHub. Your summary is kept.
      <button type="button" class="link" onclick={() => (proto.submitState = { kind: "editing" })}>OK</button>
    </p>
  {/if}

  <textarea
    value={proto.submitBody}
    oninput={(e) => (proto.submitBody = e.currentTarget.value)}
    {onkeydown}
    disabled={busy}
    placeholder="Summary (optional for Approve)"
  ></textarea>

  <div class="events" role="radiogroup" aria-label="Kind of review">
    {#each EVENTS as one (one.key)}
      <label
        class:off={ownBlocked(one.key)}
        title={ownBlocked(one.key) ? "GitHub doesn't let you approve or request changes on your own PR." : one.help}
      >
        <input
          type="radio"
          name="event"
          value={one.key}
          checked={proto.event === one.key}
          disabled={ownBlocked(one.key) || busy}
          onchange={() => (proto.event = one.key)}
        />
        <span>{one.name}</span>
        {#if layout === "dialog"}<small>{one.help}</small>{/if}
      </label>
    {/each}
  </div>

  {#if proto.submitBlocked}
    <p class="why">{proto.submitBlocked}</p>
  {/if}

  <div class="actions">
    <button
      type="button"
      class="primary"
      disabled={!!proto.submitBlocked || busy}
      title={proto.submitBlocked ?? "Ctrl+Enter"}
      onclick={() => void proto.submit()}
    >
      {busy ? "Submitting…" : "Submit review"}
    </button>
    {#if proto.pending}
      <button
        type="button"
        disabled={busy || proto.readOnly}
        onclick={() => {
          if (confirm(`Discard your Pending review and its ${String(count)} drafts? GitHub keeps no copy.`)) proto.discard();
        }}>Discard review</button
      >
    {/if}
    {#if layout === "dialog"}
      <button type="button" onclick={() => (proto.submitOpen = false)}>Cancel</button>
    {/if}
  </div>
</div>

<style>
  .submit {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font: 13px system-ui, sans-serif;
  }

  .title {
    font-weight: 600;
    font-size: 14px;
  }

  .count {
    font-size: 12px;
    opacity: 0.75;
  }

  textarea {
    min-height: 70px;
    resize: vertical;
    padding: 6px;
    font: inherit;
    color: inherit;
    background: Canvas;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 4px;
  }

  .dialog textarea {
    min-height: 120px;
  }

  .events {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .rail .events,
  .panel .events {
    flex-direction: row;
    flex-wrap: wrap;
    gap: 10px;
  }

  label {
    display: flex;
    gap: 6px;
    align-items: baseline;
    cursor: pointer;
  }

  label small {
    opacity: 0.6;
  }

  label.off {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .why {
    margin: 0;
    font-size: 12px;
    color: #9a6700;
  }

  .bad {
    margin: 0;
    font-size: 12px;
    color: #cf222e;
  }

  .actions {
    display: flex;
    gap: 6px;
  }

  .actions button {
    padding: 3px 10px;
    font: inherit;
    cursor: pointer;
  }

  .primary {
    color: #fff;
    background: #1f883d;
    border: 1px solid #1a7f37;
    border-radius: 4px;
  }

  .primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .link {
    all: unset;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
