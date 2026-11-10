<!-- PROTOTYPE, throwaway (#365): what a Definition peek holds, whichever variant places it: a header, the
     hits, and the file of the hit picked, scrolled to its line. `arrangement` lays the hits beside the code
     (A), in a column to its left (B) or in a strip over it (C). -->
<script lang="ts">
  import { commitWords, fileKey, proto, type Peek } from "./definition-peek.svelte";
  import PeekCode from "./PeekCode.svelte";

  let { peek, arrangement }: { peek: Peek; arrangement: "right" | "left" | "top" } = $props();
  const onopen = () => {
    proto.openSelected();
  };

  const hits = $derived(peek.answer.state === "done" ? peek.answer.hits : undefined);
  const hit = $derived(hits?.hits[peek.selected]);
  const many = $derived((hits?.hits.length ?? 0) > 1);
  const kindWords: Record<string, string> = { python: "Python", c: "C", rest: "reST", word: "word" };
  const countWords = $derived.by(() => {
    if (!hits) return "";
    const n = hits.hits.length;
    const noun = hits.fallback ? (n === 1 ? "whole-word match" : "whole-word matches") : n === 1 ? "definition" : "definitions";
    return `${String(n)}${hits.capped ? "+" : ""} ${noun} · ${String(hits.millis)} ms`;
  });
</script>

{#snippet list()}
  <ol class="hits" role="listbox" aria-label="Definitions" tabindex="0" data-proto-hits>
    {#each hits?.hits ?? [] as one, at (at)}
      <li
        role="option"
        aria-selected={at === peek.selected}
        class:selected={at === peek.selected}
        onclick={() => {
          proto.select(at);
        }}
        ondblclick={onopen}
      >
        <span class="where">{one.path.split("/").pop()}<span class="line">:{one.line}</span></span>
        <span class="dir">{one.path.includes("/") ? one.path.slice(0, one.path.lastIndexOf("/")) : ""}</span>
        <span class="snippet">{one.text}</span>
      </li>
    {/each}
  </ol>
{/snippet}

<div class={`body ${arrangement}`} data-proto-peek>
  <header>
    <strong class="word">{peek.lookup.word}</strong>
    {#if peek.trail.length > 0}
      <span class="trail" title="Alt+F12 inside the peek replaced these">
        {#each peek.trail as was, at (at)}<span>{was.word}</span> › {/each}
      </span>
    {/if}
    {#if hit}
      <span class="path">{hit.path}:{hit.line}</span>
    {/if}
    <span class="meta">at {commitWords(peek.lookup)}</span>
    <span class="meta">{countWords}</span>
    {#if hits?.fallback}<span class="badge">No definition found, so whole-word matches</span>{/if}
    <span class="spring"></span>
    {#if hit}<button type="button" onclick={onopen} title="Go to definition (F12, Enter)">Open</button>{/if}
    <button type="button" aria-label="Close" title="Close (Esc)" onclick={() => proto.closePeek()}>✕</button>
  </header>
  {#if peek.answer.state === "searching"}
    <p class="note">{`Searching for the definition of ${peek.lookup.word} at ${peek.lookup.commit.slice(0, 7)}…`}</p>
  {:else if peek.answer.state === "cloning"}
    <p class="note">
      {`Definitions wait for the Repository's mirror. Cloning: ${String(proto.cloned)}% (1.1 of 2.6 GB), about 3 minutes left.`}
      <br />The peek fills in once the clone ends, if it's still open.
    </p>
  {:else if peek.answer.state === "error"}
    <p class="note" role="alert">{peek.answer.error}</p>
  {:else if !hit}
    <p class="note">{`${peek.lookup.word} isn't defined, or even mentioned as a whole word, at ${peek.lookup.commit.slice(0, 7)}.`}</p>
  {:else}
    <div class="panes">
      {#if many && arrangement !== "right"}{@render list()}{/if}
      <div class="code">
        <PeekCode file={proto.files.get(fileKey(peek.lookup.commit, hit.path))} line={hit.line} />
      </div>
      {#if many && arrangement === "right"}{@render list()}{/if}
    </div>
  {/if}
</div>

<style>
  .body {
    display: flex;
    flex-direction: column;
    height: 100%;
    font-family: system-ui, sans-serif;
    font-size: 12px;
  }

  header {
    display: flex;
    flex: none;
    align-items: baseline;
    gap: 10px;
    padding: 3px 8px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    background: color-mix(in srgb, #0969da 8%, transparent);
    white-space: nowrap;
    overflow: hidden;
  }

  .word,
  .path {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  }

  .meta,
  .trail {
    opacity: 0.7;
  }

  .badge {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, #d4a72c 35%, transparent);
  }

  .spring {
    flex: 1;
  }

  header button {
    padding: 0 6px;
    border: 0;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
  }

  header button:hover {
    background: color-mix(in srgb, #0969da 20%, transparent);
  }

  .note {
    margin: 10px 12px;
    opacity: 0.8;
  }

  .panes {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .top .panes {
    flex-direction: column;
  }

  .code {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .hits {
    flex: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .right .hits {
    width: 280px;
    border-left: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .left .hits {
    width: 300px;
    border-right: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .top .hits {
    display: flex;
    max-height: 26px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    overflow-x: auto;
    overflow-y: hidden;
  }

  .hits li {
    display: flex;
    flex-direction: column;
    padding: 3px 8px;
    cursor: pointer;
  }

  .top .hits li {
    flex-direction: row;
    gap: 6px;
    white-space: nowrap;
  }

  .top .hits .dir,
  .top .hits .snippet {
    display: none;
  }

  .hits li.selected {
    background: color-mix(in srgb, #0969da 22%, transparent);
  }

  .hits:focus-visible {
    outline: 1px solid currentColor;
  }

  .where {
    font-weight: 600;
  }

  .line {
    font-weight: normal;
    opacity: 0.7;
  }

  .dir,
  .snippet {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.7;
  }

  .snippet {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  }
</style>
