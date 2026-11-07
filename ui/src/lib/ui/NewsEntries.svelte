<!-- A PR's NEWS entries, rendered above its diff as CPython's changelog shows them (#181). The core turns
     each entry's reStructuredText into blocks and pieces, so nothing here parses markup. -->
<script lang="ts">
  import type { NewsBlock, NewsEntry, NewsPiece } from "$lib/state/pr-files.svelte";
  import { address } from "./news-links";

  let { news, onlink }: { news: NewsEntry[]; onlink: (address: string) => void } = $props();

  /** Follows a link in the system browser, as a link in a description does. Enter on a link arrives as a
   * click, and a middle click as an auxclick, which would otherwise ask the webview for a window. */
  function followLink(e: MouseEvent) {
    // Only an element takes a click.
    const link = (e.target as Element).closest("a");
    if (!link) return;
    e.preventDefault();
    onlink(link.href);
  }
</script>

<!-- eslint-disable svelte/no-navigation-without-resolve -- Each link opens in the system browser through
     onlink, and none moves the window. -->

<!-- prettier-ignore -->
{#snippet pieces(runs: NewsPiece[])}{#each runs as piece, at (at)}{#if piece.kind === "emphasis"}<em>{piece.text}</em>{:else if piece.kind === "strong"}<strong>{piece.text}</strong>{:else if piece.kind === "code"}<code>{piece.text}</code>{:else if piece.kind === "link"}<a href={address(piece.to)}>{piece.text}</a>{:else}{piece.text}{/if}{/each}{/snippet}

{#snippet blocks(list: NewsBlock[])}
  {#each list as block, at (at)}
    {#if block.kind === "paragraph"}
      <p>{@render pieces(block.pieces)}</p>
    {:else if block.kind === "list"}
      <ul>
        {#each block.items as item, place (place)}
          <li>{@render blocks(item)}</li>
        {/each}
      </ul>
    {:else}
      <pre><code>{block.text}</code></pre>
    {/if}
  {/each}
{/snippet}

{#if news.length > 0}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <section
    class="news"
    aria-label="NEWS entries"
    onclick={followLink}
    onauxclick={(e) => {
      // A right click opens a menu, and the side buttons go back and forward.
      if (e.button === 1) followLink(e);
    }}
  >
    {#each news as entry (entry.path)}
      <article aria-label={`NEWS entry in ${entry.section}`}>
        <p class="about">
          <span class="section">{entry.section}</span>
          {#if entry.issue}
            <a href={address(entry.issue.to)}>{entry.issue.text}</a>
          {/if}
        </p>
        {@render blocks(entry.blocks)}
      </article>
    {/each}
  </section>
{/if}

<style>
  .news {
    padding: 8px 12px;
    font-family: system-ui, sans-serif;
    font-size: 13px;
    white-space: normal;
    border-bottom: 1px solid color-mix(in srgb, currentColor 20%, transparent);
  }

  article + article {
    margin-top: 12px;
  }

  .about {
    display: flex;
    gap: 8px;
    margin: 0 0 4px;
    font-size: 12px;
  }

  .section {
    font-weight: 600;
  }

  p,
  ul {
    margin: 4px 0;
  }

  code {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
  }

  pre {
    overflow-x: auto;
    margin: 4px 0;
    padding: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
  }
</style>
