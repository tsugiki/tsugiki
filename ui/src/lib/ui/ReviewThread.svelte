<!-- A review thread under its line in the Files tab (#184): who started it and how many comments it has,
     then its comments oldest first, or that one line alone once folded. A hidden comment folds to why
     GitHub hid it, and opens on a click. -->
<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import type { DiffThread } from "$lib/state/pr-files.svelte";
  import { REASONS } from "./hidden";
  import RenderedHtml from "./RenderedHtml.svelte";
  import { ago } from "./workbench-row";

  let {
    thread,
    folded,
    now,
    ontoggle,
    onlink,
  }: {
    thread: DiffThread;
    folded: boolean;
    now: Date;
    ontoggle: () => void;
    onlink: (address: string) => void;
  } = $props();

  /** The hidden comments the user opened, by their place in the thread. */
  const opened = new SvelteSet<number>();

  const count = $derived(thread.comments.length + thread.later_comments);
  const started = $derived(thread.comments.at(0)?.author ?? "deleted user");
  const summary = $derived(`${started} · ${count === 1 ? "1 comment" : `${String(count)} comments`}`);
  const label = $derived(thread.resolution === "resolved" ? `${summary}, resolved` : summary);
  const later = (count: number) =>
    count === 1 ? "1 later comment is on GitHub." : `${String(count)} later comments are on GitHub.`;
</script>

<button type="button" class="head" aria-expanded={!folded} aria-label={label} onclick={ontoggle}>
  <span class="chevron">{folded ? "▸" : "▾"}</span><span>{summary}</span
  >{#if thread.resolution === "resolved"}<span class="mark">Resolved</span>{/if}
</button>
{#if !folded}
  {#each thread.comments as comment, at (at)}
    <article>
      <p class="by">{`${comment.author ?? "deleted user"} · ${ago(comment.created_at, now)}`}</p>
      {#if comment.hidden === null}
        <RenderedHtml html={comment.body_html} {onlink} />
      {:else}
        <button
          type="button"
          class="fold"
          aria-expanded={opened.has(at)}
          onclick={() => {
            if (!opened.delete(at)) opened.add(at);
          }}>{REASONS[comment.hidden]}</button
        >
        {#if opened.has(at)}
          <RenderedHtml html={comment.body_html} {onlink} />
        {/if}
      {/if}
    </article>
  {/each}
  {#if thread.later_comments > 0}
    <p class="by">{later(thread.later_comments)}</p>
  {/if}
{/if}

<style>
  .head {
    display: flex;
    gap: 6px;
    align-items: center;
    width: 100%;
    height: 20px;
    padding: 0 8px;
    border: 0;
    font: inherit;
    color: inherit;
    text-align: left;
    background: none;
    cursor: pointer;
  }

  .head:hover,
  .head:focus-visible {
    text-decoration: underline;
  }

  .chevron {
    width: 1ch;
  }

  .mark {
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 4px;
    padding: 0 4px;
    line-height: 16px;
  }

  article {
    padding: 0 8px 4px;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }

  .by {
    margin: 4px 0;
    opacity: 0.75;
  }

  .fold {
    all: unset;
    cursor: pointer;
    display: block;
    margin: 4px 0;
    opacity: 0.75;
  }

  .fold:hover,
  .fold:focus-visible {
    text-decoration: underline;
  }
</style>
