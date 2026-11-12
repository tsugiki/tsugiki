<!-- The slim bar over the Detail pane (#586): ‹ and › through what the pane showed, greyed out when there's
     nothing that way, and the title of the item on show. A right-click on an arrow lists the recent items
     that way, the nearest first. -->
<script lang="ts">
  import { tick } from "svelte";
  import { currentOs, keyLabel } from "./keys";

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
    /** The title of the item on show. */
    current: string;
    /** Goes `steps` items away, back for a negative number. */
    ongo: (steps: number) => void;
  } = $props();

  /** Which arrow's list shows, if one does: -1 for back, 1 for forward. */
  let menu = $state<-1 | 1>();
  let list = $state<HTMLElement>();
  let bar = $state<HTMLElement>();
  const os = currentOs();

  const titles = (way: -1 | 1) => (way < 0 ? back : forward);
  const tooltip = (way: -1 | 1) => {
    const target = titles(way)[0];
    const word = way < 0 ? "Back" : "Forward";
    const key = keyLabel(way < 0 ? "alt+arrowleft" : "alt+arrowright", os);
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
  /** The list's own keys: ↑ and ↓ move between its items, Home and End go to either end, and Esc closes it.
   * No key goes on to the window, whose ↑ and ↓ would move the Workbench's cursor behind the list. */
  const menuKey = (e: KeyboardEvent & { currentTarget: HTMLElement }) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      menu = undefined;
      return;
    }
    const items = [...e.currentTarget.querySelectorAll("button")];
    const at = items.findIndex((item) => item === document.activeElement);
    const moves: Partial<Record<string, number>> = {
      ArrowDown: at + 1,
      ArrowUp: at - 1,
      Home: 0,
      End: items.length - 1,
    };
    const to = moves[e.key];
    if (to === undefined) return;
    e.preventDefault();
    items[Math.min(Math.max(to, 0), items.length - 1)]?.focus();
  };
</script>

<!-- A click, or a right-click, anywhere but the bar closes the list. -->
<svelte:window
  onclick={() => {
    menu = undefined;
  }}
  oncontextmenu={(e) => {
    if (!(e.target instanceof Node && bar?.contains(e.target))) menu = undefined;
  }}
/>

<nav class="history" aria-label="History" bind:this={bar}>
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
  <span class="current">{current}</span>
  {#if menu !== undefined}
    {@const way = menu}
    <div
      class="menu"
      class:ahead={way > 0}
      role="menu"
      tabindex="-1"
      aria-label={way < 0 ? "Recent items back" : "Recent items forward"}
      bind:this={list}
      onkeydown={menuKey}
      onfocusout={(e) => {
        // Tab, or focus taken by a key, leaves the list, and closes it. A right-click on an arrow focuses
        // the arrow first, and its own handler then closes or switches the list.
        if (!(e.relatedTarget instanceof Node && bar?.contains(e.relatedTarget))) menu = undefined;
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
