<!-- The Definition peek (#438), under the line of the word the user picked: a header with the symbol, the
     path and line of the hit it shows, the commit it is at, which hit it is of how many, and ✕. Under it, the
     hit's code scrolled to its line (#439), with the hits listed on its right (#441), or else a line that
     says what the lookup waits for, with the seconds once they pass one (#440), that it found nothing, or why
     it failed. A hit outside the Repository names its whole path on disk in place of a commit. While the
     server that answered builds its index, a line under the header says so (#680). -->

<script lang="ts">
  import { peekWords, type Peek } from "$lib/state/peek";
  import { currentOs, keyName } from "./keys";
  import PeekCode from "./PeekCode.svelte";

  let { peek, onclose, onhit }: { peek: Peek; onclose: () => void; onhit: (index: number) => void } =
    $props();

  /** The time the seconds of a lookup that waits count to. */
  let now = $state(Date.now());
  $effect(() => {
    // Only a lookup that asks says its seconds.
    if (peek.lookup.state !== "looking" || peek.lookup.waiting?.step !== "asking") return;
    now = Date.now();
    const clock = setInterval(() => {
      now = Date.now();
    }, 250);
    return () => {
      clearInterval(clock);
    };
  });
  const words = $derived(peekWords(peek, now));
  const code = $derived(peek.code);
  const found = $derived(words.found);
  /** Names the list's rows, so the list can say which one the peek shows. */
  const id = $props.id();
  const rowId = (index: number) => `${id}-hit-${String(index)}`;

  /** The height of each row of the list: two lines of 18 px, and 2 px above and below. */
  const HIT_ROW = 40;

  /** Scrolls `list` as little as it takes to show the row of hit `shown`. `scrollIntoView` would scroll the
   * diff around the peek too. */
  function keepInSight(list: HTMLElement, shown: number) {
    const top = shown * HIT_ROW;
    if (top < list.scrollTop) list.scrollTop = top;
    else list.scrollTop = Math.max(list.scrollTop, top + HIT_ROW - list.clientHeight);
  }

  /** While the peek shows hit `shown` and has the focus, ↑ and ↓ move through its hits, and the diff's keys
   * never hear them. */
  function moveByKey(e: KeyboardEvent, shown: number) {
    const key = keyName(e, currentOs());
    if (key !== "arrowdown" && key !== "arrowup") return;
    e.preventDefault();
    e.stopPropagation();
    onhit(shown + (key === "arrowdown" ? 1 : -1));
  }
</script>

<div class="header">
  <code class="symbol">{peek.symbol}</code>
  {#if words.where !== undefined}
    <span class="path">{words.where}</span>
  {/if}
  {#if words.commit !== undefined}
    <span class="muted">{words.commit}</span>
  {/if}
  {#if words.count !== undefined}
    <span class="muted">{words.count}</span>
  {/if}
  <button type="button" class="close" aria-label="Close" title="Close (Escape)" onclick={onclose}>✕</button>
</div>
{#if words.indexing !== undefined}
  <div class="body indexing">{words.indexing}</div>
{/if}
{#if code && found}
  <!-- The code and the list each take the focus, and ↑ and ↓ move through the hits from either. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="panes"
    class:under-indexing={words.indexing !== undefined}
    onkeydown={(e) => {
      moveByKey(e, found.shown);
    }}
  >
    <div class="code">
      <PeekCode {code} line={found.hit.line} />
    </div>
    <!-- Beside a file that can't be read too: the focus may be on it, and in 2 rows its first line shows. -->
    <ol
      class="hits"
      role="listbox"
      aria-label="Definitions"
      aria-activedescendant={rowId(found.shown)}
      tabindex="0"
      {@attach (list) => {
        keepInSight(list, found.shown);
      }}
    >
      {#each found.hits as hit, index (index)}
        <!-- ↑ and ↓ move through the list from the peek's own keys, so a click is the mouse's way. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={rowId(index)}
          role="option"
          aria-selected={index === found.shown}
          onclick={() => {
            onhit(index);
          }}
        >
          <span class="where">{hit.place.path}<span class="muted">{`:${String(hit.line)}`}</span></span>
          <code class="text">{hit.text.trim()}</code>
        </li>
      {/each}
    </ol>
  </div>
{:else}
  <div class="body">{words.body}</div>
{/if}

<style>
  .header,
  .body {
    display: flex;
    gap: 8px;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
  }

  .header {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  .symbol {
    font-weight: 600;
  }

  .muted {
    opacity: 0.7;
  }

  .close {
    margin-left: auto;
    padding: 0 4px;
    border: none;
    background: none;
    color: inherit;
    cursor: pointer;
  }

  /* The rows under the header: the code, which scrolls inside them, and the list of hits on its right. */
  .panes {
    display: flex;
    height: calc(100% - 20px);
  }

  /* The line that says the server indexes takes a row of the code's and the list's. */
  .panes.under-indexing {
    height: calc(100% - 40px);
  }

  .indexing {
    opacity: 0.7;
    font-style: italic;
  }

  .code {
    flex: 1;
    min-width: 0;
  }

  .hits {
    flex: none;
    width: 280px;
    max-width: 40%;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
    border-left: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .hits:focus-visible {
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  /* Each hit takes two lines: its path and line, then the line's text. Its height is `HIT_ROW`, which
     scrolls the list. */
  .hits li {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    height: 40px;
    padding: 2px 8px;
    line-height: 18px;
    white-space: nowrap;
    cursor: pointer;
  }

  .hits li:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  /* The hit shown, on the inactive selection until the peek holds the focus and ↑ ↓ move it. */
  .hits li[aria-selected="true"] {
    background: var(--selection-inactive);
  }

  .panes:focus-within li[aria-selected="true"] {
    background: var(--selection-active);
  }

  .where,
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .text {
    opacity: 0.7;
  }
</style>
