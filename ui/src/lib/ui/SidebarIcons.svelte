<!--
  The Sidebar shrunk to a strip of icons (#587): the Repository's initials on top, then an icon for each entry
  the full Sidebar lists, with its count as a badge and its name in the tooltip, and ⚙ at the bottom. A click
  on an entry's icon brings the full Sidebar back on that view, and a click on the initials brings it back as
  it was. The PR view's rail (#603) is another thing.
-->
<script lang="ts" module>
  /** The Repository's initials atop the strip, from the part of `repository` after its owner: the first
   * letters of its first two words, or the first two letters of its one word, so `.github` gives "GI". A
   * name of only marks has no words, and gives its first two marks. */
  export function initials(repository: string): string {
    const name = repository.slice(repository.indexOf("/") + 1);
    const [first, second] = name.split(/[-_.\s]+/).filter((word) => word !== "");
    const letters =
      first === undefined
        ? name.slice(0, 2)
        : second === undefined
          ? first.slice(0, 2)
          : first.charAt(0) + second.charAt(0);
    return letters.toUpperCase();
  }

  /** The words a badge shows for `count`, or none for nothing to count. */
  export const badge = (count: number): string | undefined =>
    count === 0 ? undefined : count > 99 ? "99+" : String(count);
</script>

<script lang="ts">
  import type { FilterLine, SidebarView } from "$lib/state/workbench.svelte";
  import { currentOs, withKey } from "./keys";
  import { currentView } from "./sidebar-view";

  let {
    repository,
    onfull,
    onsettings,
    count,
    reviewRequests,
    staleReviews,
    filters,
    person,
    onopenperson = () => {},
    pinned,
    selected,
    onselect,
  }: {
    /** The open Repository's name. */
    repository: string;
    /** Brings the full Sidebar back as it was, from the Repository's initials. */
    onfull: () => void;
    /** Opens the Settings dialog. */
    onsettings: () => void;
    count: number;
    reviewRequests: number;
    staleReviews: number;
    filters: FilterLine[];
    /** The person Filter a clicked name opened, unsaved (#472). */
    person?: { login: string } | undefined;
    /** Opens the person Filter again, which sends its search to GitHub anew (#472). */
    onopenperson?: (login: string) => void;
    /** The count of Pinned issues that Need attention (#489). */
    pinned: number;
    selected: SidebarView;
    /** Brings the full Sidebar back and shows `view`. */
    onselect: (view: SidebarView) => void;
  } = $props();

  /** Line icons on a 16 px grid, which the prototype drew for the strip. */
  const ICONS = {
    workbench: "M2 9h3l1 2h4l1-2h3M2 9l2-6h8l2 6v4H2z",
    // An open Issue's ring and dot, which the prototype drew for All issues, whose place the Inbox took.
    inbox: "M2 8a6 6 0 1 0 12 0 6 6 0 1 0-12 0M7.4 8a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0",
    review_requests: "M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5zM6 8a2 2 0 1 0 4 0 2 2 0 1 0-4 0",
    stale_reviews: "M2 8a6 6 0 1 0 12 0 6 6 0 1 0-12 0M8 4.5V8l2.5 1.5",
    archived: "M2 3h12v3H2zM3 6v7h10V6M6.5 9h3",
    filter: "M2 3h12L9.5 8.5V13l-3 1.5V8.5z",
    pinned: "M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z",
  };
</script>

<!-- One entry: its icon, with its count as a badge if it has one, and its name for the tooltip and a screen
     reader. A Filter whose last search failed says why there too, as the full Sidebar does under its line,
     and marks its icon with ! in the danger colour in place of the count, which may be short, so neither the
     tooltip nor the label gives the count. -->
{#snippet entry(icon: string, name: string, view: SidebarView, entryCount = 0, problem?: string | null)}
  {@const words = badge(entryCount)}
  {@const counted = words === undefined ? name : `${name} (${String(entryCount)})`}
  <button
    class="entry"
    aria-current={currentView(selected, view)}
    aria-label={problem ? `${name}. ${problem}` : counted}
    title={problem ? `${name}\n${problem}` : name}
    onclick={() => {
      onselect(view);
    }}
  >
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d={icon} /></svg>
    {#if problem}
      <span class="badge problem">!</span>
    {:else if words !== undefined}
      <span class="badge">{words}</span>
    {/if}
  </button>
{/snippet}

<nav aria-label="Sidebar" class="icons">
  <button class="initials mono" aria-label="Full Sidebar" title={repository} onclick={onfull}
    >{initials(repository)}</button
  >
  {@render entry(ICONS.workbench, "Workbench", { kind: "whole" }, count)}
  <!-- The Inbox and Archived have no count: an item that comes to Need attention leaves Archived (#992). -->
  {@render entry(ICONS.inbox, "Inbox", { kind: "inbox" })}
  <span class="rule"></span>
  {@render entry(ICONS.review_requests, "Review requests", { kind: "review_requests" }, reviewRequests)}
  {@render entry(ICONS.stale_reviews, "Stale reviews", { kind: "stale_reviews" }, staleReviews)}
  {@render entry(ICONS.archived, "Archived", { kind: "archived" })}
  {#if filters.length > 0}
    <span class="rule"></span>
  {/if}
  <!-- Two Filters may share a search, so a search can't be the key. -->
  {#each filters as filter, index (index)}
    {@render entry(
      ICONS.filter,
      filter.name,
      { kind: "filter", query: filter.query },
      filter.count,
      filter.problem,
    )}
  {/each}
  <!-- The unsaved person Filter, which the full Sidebar marks and offers Save and Close (#472). -->
  {#if person}
    {@const login = person.login}
    <button
      class="entry"
      aria-current={currentView(selected, { kind: "person", login })}
      aria-label={`${login}, unsaved`}
      title={`${login} (unsaved)`}
      onclick={() => onopenperson(login)}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d={ICONS.filter} /></svg>
    </button>
  {/if}
  <span class="rule"></span>
  {@render entry(ICONS.pinned, "Pinned issues", { kind: "pinned" }, pinned)}
  <span class="grow"></span>
  <button
    class="entry"
    aria-haspopup="dialog"
    aria-label="Settings"
    title={withKey("Settings", "open_settings", currentOs())}
    onclick={onsettings}>⚙</button
  >
</nav>

<style>
  /* The Sidebar's panel colour and border, at a width that holds one icon. */
  nav {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: center;
    align-self: stretch;
    gap: 2px;
    box-sizing: border-box;
    width: 48px;
    padding: 6px 0;
    border-right: 1px solid var(--border);
    overflow: hidden auto;
    overscroll-behavior: contain;
    font-size: 13px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
  }

  button {
    all: unset;
    position: relative;
    display: grid;
    flex: none;
    place-items: center;
    width: 34px;
    height: 30px;
    border-radius: 6px;
    cursor: pointer;
  }

  button:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  button:focus-visible {
    outline: 1px solid var(--accent);
  }

  .entry {
    color: var(--fg-muted);
  }

  .entry:hover,
  [aria-current] {
    color: var(--fg-chrome);
  }

  [aria-current] {
    background: var(--selected);
  }

  svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* The count, on the icon's top right corner. */
  .badge {
    position: absolute;
    top: 0;
    right: -2px;
    padding: 0 4px;
    border-radius: 7px;
    font-size: 9.5px;
    font-weight: 600;
    line-height: 13px;
    color: var(--bg);
    background: var(--accent);
  }

  .badge.problem {
    background: var(--danger);
  }

  .initials {
    box-sizing: border-box;
    width: 30px;
    height: 26px;
    margin: 2px 0 4px;
    border: 1px solid var(--border);
    font-size: 11px;
    font-weight: 700;
  }

  .rule {
    flex: none;
    width: 22px;
    margin: 4px 0;
    border-top: 1px solid var(--border);
  }

  .grow {
    flex: 1;
  }
</style>
