<!-- A review thread in the Files tab (#184): who started it and how many comments it has, then its comments
     oldest first, or that one line alone once folded. A hidden comment folds to why GitHub hid it, and opens
     on a click. A thread on the whole file, an Outdated one, one on a line no hunk shows and one on a file no
     longer in the diff say so, and an Outdated one shows the last lines of the code it was written against,
     or the whole hunk on "Show more". A draft from the user's Pending review is marked as theirs (#185). -->
<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { CODE_ROWS, type DiffThread } from "$lib/state/pr-files.svelte";
  import { REASONS } from "./hidden";
  import RenderedHtml from "./RenderedHtml.svelte";
  import ThreadCode from "./ThreadCode.svelte";
  import { ago, commentCount } from "./workbench-row";

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
  /** Whether the whole hunk shows, not only its last rows. */
  let whole = $state(false);

  const count = $derived(thread.comments.length + thread.later_comments);
  const started = $derived(thread.comments.at(0)?.author ?? "deleted user");
  const summary = $derived(`${started} · ${commentCount(count)}`);
  /** Where the thread is, when it isn't under a line of the diff. */
  const where = $derived.by(() => {
    const at = thread.at;
    switch (at.kind) {
      case "row":
        return undefined;
      case "file":
        return "On the file";
      case "outdated":
        return "Outdated";
      case "outside":
        return `Line ${String(at.line)}, outside the diff`;
      case "gone":
        return at.path;
    }
  });
  const code = $derived(thread.at.kind === "outdated" || thread.at.kind === "gone" ? thread.at.code : null);
  const marks = $derived([where, thread.resolution === "resolved" ? "Resolved" : undefined]);
  const label = $derived([summary, ...marks.filter((mark) => mark !== undefined)].join(", "));
  const later = (count: number) =>
    count === 1 ? "1 later comment is on GitHub." : `${String(count)} later comments are on GitHub.`;
</script>

<button type="button" class="head" aria-expanded={!folded} aria-label={label} onclick={ontoggle}>
  <span class="chevron">{folded ? "▸" : "▾"}</span><span>{summary}</span
  >{#each marks as mark, at (at)}{#if mark !== undefined}<span class="mark">{mark}</span>{/if}{/each}
</button>
{#if !folded}
  {#if code !== null}
    <ThreadCode
      rows={whole ? code.rows : code.rows.slice(-CODE_ROWS)}
      spans={whole ? code.spans : code.spans.slice(-CODE_ROWS)}
    />
    {#if code.rows.length > CODE_ROWS}
      <button
        type="button"
        class="fold more"
        aria-expanded={whole}
        onclick={() => {
          whole = !whole;
        }}>{whole ? "Show less" : "Show more"}</button
      >
    {/if}
  {/if}
  {#each thread.comments as comment, at (at)}
    <article>
      <p class="by">
        {`${comment.author ?? "deleted user"} · ${ago(comment.created_at, now)}`}{#if comment.submission === "pending"}<span
            class="pending"
            title="Only you see it until you submit your review on github.com.">Pending, yours</span
          >{/if}
      </p>
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

  /* github.com's attention color, for a draft only the user sees. */
  .pending {
    margin-left: 6px;
    border: 1px solid #9a6700;
    border-radius: 4px;
    padding: 0 4px;
    color: #9a6700;
  }

  :global(:root[data-theme="dark"]) .pending {
    border-color: #d29922;
    color: #d29922;
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

  .more {
    margin: 0 8px 4px;
  }

  .fold:hover,
  .fold:focus-visible {
    text-decoration: underline;
  }
</style>
