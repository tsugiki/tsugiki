<!--
  The rail on the right of the PR view, beside the Conversation and Files tabs (#603). It holds a pane for
  each of its tabs (#727): Files for now, which the tree of the PR's changed files fills (#604). A row of
  tabs at its top picks the pane, once it has two tabs or more. Its edge sits inside it, on its left. × in
  its corner closes it, and Esc in it hands the focus back to what it sits beside.
-->
<script lang="ts" generics="T extends string">
  import { type Snippet, tick, untrack } from "svelte";
  import { railTabLabel, type RailState, type RailStep, type RailTab } from "$lib/state/rail.svelte";
  import { currentOs, keyName, withKey } from "./keys";

  let {
    tabs,
    rail,
    edge,
    onleave,
  }: {
    /** The rail's tabs, by what `rail` calls them. */
    tabs: Readonly<Record<T, RailTab>>;
    /** Whether the rail shows, and which tab, of those in the order the tab row shows them. */
    rail: RailState<T>;
    /** The edge that sizes the rail, on its left. */
    edge: Snippet;
    /** Gives the focus back to the diff or the Conversation, as Esc in the rail does, and as the rail goes
     * while it holds the focus. */
    onleave: () => void;
  } = $props();

  /** The tab on show: the one picked last. */
  const shown = $derived(tabs[rail.tab]);
  /** Whether the rail shows a tab row: only while it has more than one tab to pick. */
  const tabbed = $derived(rail.order.length > 1);
  /** The tab row. */
  let tabRow = $state<HTMLElement | null>(null);
  /** Whether the focus is in the rail, from the first focus in it until the focus moves to an element outside
   * it, or to the page, as a click on a pane's background moves it. Another app taking the focus leaves the
   * rail holding it, as the window gives it back there. */
  let held = false;

  /** Esc, in the rail or on its edge, leaves the rail open and goes back to what it sits beside. It goes no
   * further, so the window doesn't also leave the PR for its Issue. */
  function onkeydown(e: KeyboardEvent) {
    if (keyName(e, currentOs()) !== "escape") return;
    e.preventDefault();
    e.stopPropagation();
    onleave();
  }

  const selectedTab = () => tabRow?.querySelector<HTMLElement>('[aria-selected="true"]');

  /** The tab each key of the tab row moves to, as the ARIA tabs pattern has it. */
  const STEPS: Readonly<Record<string, RailStep>> = {
    home: "first",
    arrowleft: "previous",
    arrowright: "next",
    end: "last",
  };

  /** Home, End, ← and → in the tab row show the first tab, the one before the tab on show, the one after it,
   * or the last, round from either end, and focus it. */
  async function ontabkeydown(e: KeyboardEvent) {
    const step = STEPS[keyName(e, currentOs()) ?? ""];
    if (step === undefined) return;
    e.preventDefault();
    rail.go(step);
    await tick();
    selectedTab()?.focus();
  }

  /** A click on a tab shows it, and a click on the tab on show closes the rail (#727). Enter or Space on the
   * tab on show, or a screen reader's press, leaves the rail open: the ARIA tabs pattern lets them confirm
   * the tab. Only a mouse's click counts clicks in `detail`. */
  function ontabclick(id: T, e: MouseEvent) {
    if (e.detail === 0) rail.pick(id);
    else rail.toggle(id);
  }

  // A key or a button that shows another tab while the focus is in the pane on show takes that pane away,
  // and the focus would fall to the page, where ↑ ↓ move the Issue list's cursor. The tab on show takes it.
  $effect(() => {
    void rail.tab;
    untrack(() => {
      if (held && document.activeElement === document.body) selectedTab()?.focus();
    });
  });

  /** Hands the focus on as the rail goes while it holds the focus, however it closes: the focus would fall
   * to the page. By the time the rail goes, the page has taken it off the screen, and the tab beside it has
   * taken its place, which `onleave` then finds. */
  function handOnFocus(aside: HTMLElement) {
    aside.addEventListener("focusin", () => {
      held = true;
    });
    aside.addEventListener("focusout", (e) => {
      if (e.relatedTarget instanceof Node) {
        if (!aside.contains(e.relatedTarget)) held = false;
        return;
      }
      // A browser may say the focus left as the rail goes, which the rail's going then answers first, or as
      // the pane on show goes, after which the tab on show may have taken the focus back.
      queueMicrotask(() => {
        if (aside.isConnected && !aside.contains(document.activeElement) && document.hasFocus()) held = false;
      });
    });
    return () => {
      if (held && document.activeElement === document.body) queueMicrotask(onleave);
    };
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="rail" aria-label="Rail" data-rail {onkeydown} {@attach handOnFocus}>
  {@render edge()}
  <div class="column">
    <header>
      {#if tabbed}
        <div class="tabs" role="tablist" aria-label="Rail tabs" bind:this={tabRow}>
          {#each rail.order as id (id)}
            {@const label = railTabLabel(tabs[id])}
            <button
              type="button"
              role="tab"
              id={`rail-tab-${id}`}
              aria-selected={id === rail.tab}
              aria-controls="rail-pane"
              tabindex={id === rail.tab ? 0 : -1}
              onkeydown={ontabkeydown}
              title={withKey(label, tabs[id].action, currentOs())}
              onclick={(e) => {
                ontabclick(id, e);
              }}>{label}</button
            >
          {/each}
        </div>
      {:else}
        <h3 class="small-heading">{railTabLabel(shown)}</h3>
      {/if}
      <button
        type="button"
        class="close"
        aria-label="Close the rail"
        title={withKey("Close", shown.action, currentOs())}
        onclick={() => {
          rail.close();
        }}>×</button
      >
    </header>
    <div
      class="pane"
      id={tabbed ? "rail-pane" : undefined}
      role={tabbed ? "tabpanel" : "region"}
      aria-labelledby={tabbed ? `rail-tab-${rail.tab}` : undefined}
      aria-label={tabbed ? undefined : shown.name}
    >
      {@render shown.pane()}
    </div>
  </div>
</aside>

<style>
  /* A step off the content, with a border, as the Sidebar is. The edge on its left measures the whole box,
     border and edge too, and gives that width back. It gives way to the content in a narrow pane. */
  .rail {
    display: flex;
    flex: 0 1 var(--rail-width);
    box-sizing: border-box;
    min-width: 0;
    border-left: 1px solid var(--border);
    font-size: 13px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
  }

  .column {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 4px 0;
  }

  h3 {
    flex: 1;
    margin: 0;
  }

  /* The tab on show underlined in the accent colour, as the PR's own tabs are. */
  .tabs {
    display: flex;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  [role="tab"] {
    all: unset;
    cursor: pointer;
    padding: 2px 8px;
    font-size: 12px;
    white-space: nowrap;
    border-bottom: 2px solid transparent;
  }

  [role="tab"][aria-selected="true"] {
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  [role="tab"]:focus-visible {
    outline: 1px solid var(--focus-border);
  }

  .close {
    all: unset;
    width: 22px;
    line-height: 22px;
    text-align: center;
    border-radius: 4px;
    cursor: pointer;
  }

  .close:hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .close:focus-visible {
    outline: 1px solid var(--accent);
  }

  .pane {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
</style>
