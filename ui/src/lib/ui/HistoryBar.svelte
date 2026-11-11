<!-- The slim bar over the Detail pane (#586): ‹ and › through what the pane showed, greyed out when there's
     nothing that way, and the title of the item on show. A right-click on an arrow lists the recent items
     that way, the nearest first. -->
<script lang="ts">
  import { tick } from "svelte";

  let {
    back,
    forward,
    current,
    ongo,
  }: {
    /** The titles of the items before the one on show, the nearest first. */
    back: string[];
    /** The titles of the items ahead of it, the nearest first. */
    forward: string[];
    /** The title of the item on show, once it's known. */
    current: string | undefined;
    /** Goes `steps` items away, back for a negative number. */
    ongo: (steps: number) => void;
  } = $props();

  /** Which arrow's list shows, if one does: -1 for back, 1 for forward. */
  let menu = $state<-1 | 1>();
  let list = $state<HTMLElement>();

  const titles = (way: -1 | 1) => (way < 0 ? back : forward);
  const tooltip = (way: -1 | 1) => {
    const target = titles(way)[0];
    const word = way < 0 ? "Back" : "Forward";
    const key = way < 0 ? "Alt+←" : "Alt+→";
    return target === undefined ? `${word} (${key}): nothing there` : `${word} to ${target} (${key})`;
  };
  const go = (steps: number) => {
    menu = undefined;
    ongo(steps);
  };
  const openMenu = async (e: MouseEvent, way: -1 | 1) => {
    e.preventDefault();
    menu = menu === way ? undefined : way;
    await tick();
    list?.querySelector("button")?.focus();
  };
</script>

<svelte:window
  onclick={() => {
    menu = undefined;
  }}
/>

<nav class="history" aria-label="History">
  {#each [-1, 1] as const as way (way)}
    <button
      class="arrow"
      aria-label={way < 0 ? "Back" : "Forward"}
      title={tooltip(way)}
      disabled={titles(way).length === 0}
      onclick={() => {
        go(way);
      }}
      oncontextmenu={(e) => void openMenu(e, way)}>{way < 0 ? "‹" : "›"}</button
    >
  {/each}
  <span class="current">{current ?? ""}</span>
  {#if menu !== undefined}
    {@const way = menu}
    <div
      class="menu"
      class:ahead={way > 0}
      role="menu"
      tabindex="-1"
      aria-label={way < 0 ? "Recent items back" : "Recent items forward"}
      bind:this={list}
      onkeydown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          menu = undefined;
        }
      }}
    >
      {#each titles(way) as title, index (index)}
        <button
          role="menuitem"
          onclick={() => {
            go(way * (index + 1));
          }}>{title}</button
        >
      {/each}
    </div>
  {/if}
</nav>

<style>
  /* Stays at the top of the pane while a long Issue scrolls under it. */
  .history {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    padding: 2px 0;
    font-size: 12px;
    background: var(--bg, Canvas);
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .arrow {
    all: unset;
    padding: 0 6px;
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }

  .arrow:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .arrow:not(:disabled):hover,
  .arrow:focus-visible {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .current {
    overflow: hidden;
    margin-left: 4px;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.75;
  }

  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    display: flex;
    flex-direction: column;
    max-width: 480px;
    padding: 4px 0;
    background: var(--bg-overlay, Canvas);
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 4px;
  }

  .menu.ahead {
    left: 24px;
  }

  .menu button {
    all: unset;
    overflow: hidden;
    padding: 2px 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .menu button:hover,
  .menu button:focus-visible {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }
</style>
