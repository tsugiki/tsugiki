<!-- PROTOTYPE, throwaway (#367): the composer. `layout` picks how it shows: inline under the line with
     Write and Preview tabs (A), in a panel with the lines quoted and a live preview beside the text (B), or
     in the rail with the lines quoted and a Preview toggle (C). -->
<script lang="ts">
  import { render } from "./markdown";
  import {
    BODY_LIMIT,
    bytes,
    COUNT_FROM,
    noSuggestion,
    proto,
    suggestionOf,
    targetWords,
    type Composer,
  } from "./pending-review.svelte";

  let { composer, layout }: { composer: Composer; layout: "inline" | "panel" | "rail" | "plain" } = $props();

  let previewing = $state(false);
  let box = $state<HTMLTextAreaElement>();

  const target = $derived(composer.target);
  const quoted = $derived(target.kind === "lines" ? target.lines : []);
  const replaced = $derived(quoted.filter((one) => one.kind !== "removed").map((one) => one.text));
  const noSuggest = $derived(noSuggestion(target));
  const size = $derived(bytes(composer.body));
  const tooLong = $derived(size > BODY_LIMIT);
  const busy = $derived(composer.state.kind === "sending");
  const blocked = $derived(busy || composer.state.kind === "unknown" || composer.state.kind === "moved");
  const empty = $derived(!composer.body.trim());
  const inReview = $derived(target.kind !== "conversation");
  const ro = $derived(proto.readOnly);

  $effect(() => {
    void composer.target;
    box?.focus();
  });

  function suggest() {
    if (noSuggest) {
      proto.say(noSuggest);
      return;
    }
    const block = suggestionOf(target);
    const body = composer.body;
    proto.type(body === "" || body.endsWith("\n") ? body + block : `${body}\n${block}`);
    box?.focus();
  }

  async function copyOpen() {
    try {
      await navigator.clipboard.writeText(composer.body);
    } catch {
      // The prototype doesn't care.
    }
    proto.say(`Copied. Would open PR #${String(proto.number)} on GitHub at ${targetWords(target)}; the text stays here`);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      if (ro) void copyOpen();
      else void proto.send(e.shiftKey && !proto.pending && inReview);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      proto.cancel();
    } else if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      if (e.shiftKey) proto.stretch(e.key === "ArrowUp" ? "top" : "bottom", -1);
      else proto.stretch(e.key === "ArrowUp" ? "top" : "bottom", 1);
    } else if (e.key === "g" && e.ctrlKey) {
      e.preventDefault();
      suggest();
    } else if (e.key.toLowerCase() === "v" && e.ctrlKey && e.shiftKey) {
      e.preventDefault();
      previewing = !previewing;
    }
  }

  const sign = (side: string) => (side === "LEFT" ? "−" : "+");
</script>

<div class={`composer ${layout}`} role="group" aria-label="Composer">
  <div class="where">
    <span>{composer.editing !== undefined ? "Editing your draft · " : ""}{targetWords(target)}</span>
    {#if target.kind === "lines"}
      <span class="hint">Shift+click or Alt+↑ ↓ to widen · Alt+Shift+↑ ↓ to narrow</span>
    {/if}
  </div>

  {#if layout !== "inline" && quoted.length > 0}
    <div class="quote">
      {#each quoted.slice(0, 8) as one (one.row)}
        <div class={one.kind}><span class="n">{sign(one.side)}{one.line}</span>{one.text}</div>
      {/each}
      {#if quoted.length > 8}<div class="more">… {quoted.length - 8} more lines</div>{/if}
    </div>
  {/if}

  {#if proto.closed}
    <p class="note">This PR is closed. GitHub still takes comments, and the author may not look.</p>
  {/if}

  {#if layout === "inline" || layout === "plain"}
    <div class="tabs">
      <button type="button" class:on={!previewing} onclick={() => (previewing = false)}>Write</button>
      <button type="button" class:on={previewing} onclick={() => (previewing = true)}>Preview</button>
      <span class="grow"></span>
      <button type="button" class="tool" disabled={!!noSuggest} title={noSuggest ?? "Ctrl+G: suggest a change to these lines"} onclick={suggest}>± Suggestion</button>
    </div>
  {:else}
    <div class="tabs">
      <button type="button" class="tool" disabled={!!noSuggest} title={noSuggest ?? "Ctrl+G: suggest a change to these lines"} onclick={suggest}>± Suggestion</button>
      {#if layout === "rail"}
        <button type="button" class="tool" class:on={previewing} title="Ctrl+Shift+V" onclick={() => (previewing = !previewing)}>Preview</button>
      {/if}
    </div>
  {/if}

  <div class="body" class:split={layout === "panel"}>
    {#if !(previewing && layout !== "panel")}
      <textarea
        bind:this={box}
        value={composer.body}
        oninput={(e) => {
          proto.type(e.currentTarget.value);
        }}
        {onkeydown}
        disabled={busy}
        placeholder={ro
          ? "Read-only: write it here, then copy it to GitHub."
          : target.kind === "reply"
            ? "Reply…"
            : "Leave a comment. Markdown works; Ctrl+Enter adds it."}
      ></textarea>
    {/if}
    {#if previewing || layout === "panel"}
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      <div class="preview">{@html render(composer.body, replaced)}</div>
    {/if}
  </div>

  {#if size > COUNT_FROM}
    <p class="note" class:bad={tooLong}>
      {size.toLocaleString()} of {BODY_LIMIT.toLocaleString()} bytes{tooLong ? ": too long for GitHub" : ""}
    </p>
  {/if}

  {#if composer.state.kind === "sending"}
    <p class="state">Sending…</p>
  {:else if composer.state.kind === "not_sent"}
    <p class="state bad">Not sent: {composer.state.why}. Your text is kept.</p>
  {:else if composer.state.kind === "unknown"}
    <p class="state warn">
      May have been sent · checking when the connection is back.
      <button type="button" class="link" onclick={() => void proto.check()}>(prototype: connection is back)</button>
    </p>
  {:else if composer.state.kind === "moved"}
    <p class="state warn">
      These lines changed in the newer push. Click the lines to place it again; your text comes along.
    </p>
  {:else if composer.state.kind === "refused"}
    <p class="state bad">GitHub refused: your token can't write to this Repository. Your text is kept.</p>
  {/if}

  <div class="actions">
    {#if ro}
      <button type="button" class="primary" disabled={empty} onclick={() => void copyOpen()} title="Ctrl+Enter">
        Copy and open on GitHub
      </button>
    {:else if composer.editing !== undefined}
      <button type="button" class="primary" disabled={empty || blocked || tooLong} onclick={() => void proto.send()}>Save</button>
    {:else if !inReview}
      <button type="button" class="primary" disabled={empty || blocked || tooLong} onclick={() => void proto.send()} title="Ctrl+Enter">
        Comment
      </button>
    {:else}
      <button
        type="button"
        class="primary"
        disabled={empty || blocked || tooLong}
        onclick={() => void proto.send()}
        title="Ctrl+Enter"
      >
        {composer.state.kind === "not_sent" ? "Try again" : proto.pending ? "Add to review" : "Start a review"}
      </button>
      {#if !proto.pending}
        <button
          type="button"
          disabled={empty || blocked || tooLong}
          onclick={() => void proto.send(true)}
          title="Ctrl+Shift+Enter: send this one comment now, not in a review"
        >
          Comment now
        </button>
      {:else}
        <span class="aside" title="GitHub puts every reply and thread into the open Pending review (#373)">
          joins your Pending review
        </span>
      {/if}
    {/if}
    <button type="button" onclick={() => proto.cancel()} title="Esc: the text stays for this place">Cancel</button>
  </div>
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    font: 13px system-ui, sans-serif;
    border: 1px solid #0969da;
    border-radius: 6px;
    background: Canvas;
  }

  .composer.inline {
    margin: 6px 12px 6px 96px;
  }

  .composer.plain {
    margin: 12px 0;
  }

  .composer.panel,
  .composer.rail {
    border: 0;
    border-radius: 0;
    padding: 6px 8px;
  }

  .where {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-weight: 600;
  }

  .hint {
    font-weight: normal;
    font-size: 11px;
    opacity: 0.6;
  }

  .quote {
    max-height: 9em;
    overflow: auto;
    font: 12px ui-monospace, "Cascadia Mono", Consolas, monospace;
    white-space: pre;
    border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .quote .n {
    display: inline-block;
    width: 4em;
    opacity: 0.55;
    text-align: right;
    padding-right: 8px;
  }

  .quote .added {
    background: color-mix(in srgb, #2da44e 18%, transparent);
  }

  .quote .removed {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }

  .quote .more {
    opacity: 0.6;
    padding-left: 4em;
  }

  .tabs {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .grow {
    flex: 1;
  }

  .tabs button {
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 2px 8px;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
  }

  .tabs button.on {
    border-color: color-mix(in srgb, currentColor 30%, transparent);
  }

  .tabs button.tool {
    border-color: color-mix(in srgb, currentColor 20%, transparent);
  }

  .tabs button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .body {
    display: flex;
    gap: 8px;
  }

  .body > * {
    flex: 1;
    min-width: 0;
  }

  textarea {
    min-height: 80px;
    resize: vertical;
    box-sizing: border-box;
    padding: 6px;
    font: 13px system-ui, sans-serif;
    color: inherit;
    background: Canvas;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 4px;
  }

  .split textarea {
    min-height: 120px;
  }

  .preview {
    min-height: 80px;
    padding: 0 8px;
    overflow: auto;
    border: 1px dashed color-mix(in srgb, currentColor 20%, transparent);
    border-radius: 4px;
  }

  .preview :global(.suggestion) {
    margin: 6px 0;
    font: 12px ui-monospace, Consolas, monospace;
    white-space: pre;
    border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
  }

  .preview :global(.shead) {
    padding: 2px 6px;
    font: 12px system-ui, sans-serif;
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  .preview :global(.del) {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }

  .preview :global(.add) {
    background: color-mix(in srgb, #2da44e 18%, transparent);
  }

  .preview :global(.mention),
  .preview :global(.ref) {
    color: #0969da;
    font-weight: 600;
  }

  .preview :global(.nothing) {
    opacity: 0.5;
  }

  .note,
  .state {
    margin: 0;
    font-size: 12px;
  }

  .note {
    opacity: 0.75;
  }

  .bad {
    color: #cf222e;
  }

  .warn {
    color: #9a6700;
  }

  .actions {
    display: flex;
    gap: 6px;
    align-items: center;
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

  .aside {
    font-size: 11px;
    opacity: 0.6;
  }

  .link {
    all: unset;
    cursor: pointer;
    text-decoration: underline;
    font-size: 11px;
  }
</style>
