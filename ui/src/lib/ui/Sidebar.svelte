<script lang="ts">
  import type { FilterLine, SidebarView } from "$lib/state/workbench.svelte";
  import { currentOs, withKey } from "./keys";
  import { currentView } from "./sidebar-view";

  let {
    repository,
    onpick,
    onsettings,
    onicons,
    count,
    reviewRequests,
    staleReviews,
    filters,
    person,
    onopenperson = () => {},
    oncloseperson = () => {},
    onsaveperson = () => {},
    pinned,
    selected,
    onselect,
  }: {
    /** The open Repository's name. */
    repository: string;
    /** Opens the Repository picker. */
    onpick: () => void;
    /** Opens the Settings dialog. */
    onsettings: () => void;
    /** Shrinks the Sidebar to a strip of icons, from « in its header (#587). */
    onicons: () => void;
    count: number;
    reviewRequests: number;
    staleReviews: number;
    filters: FilterLine[];
    /** The person Filter a clicked name opened, unsaved, with the search it saves as once the core spells it
     * (#472). */
    person?: { login: string; query?: string | undefined } | undefined;
    /** Opens the person Filter again, which sends its search to GitHub anew (#472). */
    onopenperson?: (login: string) => void;
    /** Closes the person Filter, which leaves the Sidebar (#472). */
    oncloseperson?: () => void;
    /** Opens the Save as Filter dialog for the person Filter, with its login and query (#472). */
    onsaveperson?: ((draft: { name: string; query: string }) => void) | undefined;
    /** The count of Pinned issues that Need attention (#489). */
    pinned: number;
    selected: SidebarView;
    onselect: (view: SidebarView) => void;
  } = $props();

  /** The `aria-current` of the line for `view`. */
  const current = (view: SidebarView) => currentView(selected, view);
</script>

<nav aria-label="Sidebar">
  <div class="top">
    <button class="repository" aria-haspopup="dialog" onclick={onpick}>{repository}</button>
    <button
      class="settings"
      aria-haspopup="dialog"
      aria-label="Settings"
      title={withKey("Settings", "open_settings", currentOs())}
      onclick={onsettings}>⚙</button
    >
    <button class="fold" aria-label="Sidebar as Icons" title="Sidebar as Icons" onclick={onicons}>«</button>
  </div>
  <button aria-current={current({ kind: "whole" })} onclick={() => onselect({ kind: "whole" })}>
    <span>Workbench</span>
    <span>{count}</span>
  </button>
  <!-- The Inbox has no count. -->
  <button aria-current={current({ kind: "inbox" })} onclick={() => onselect({ kind: "inbox" })}>
    <span>Inbox</span>
  </button>
  <h2 class="small-heading">Built-in views</h2>
  <button
    aria-current={current({ kind: "review_requests" })}
    onclick={() => onselect({ kind: "review_requests" })}
  >
    <span>Review requests</span>
    <span>{reviewRequests}</span>
  </button>
  <button
    aria-current={current({ kind: "stale_reviews" })}
    onclick={() => onselect({ kind: "stale_reviews" })}
  >
    <span>Stale reviews</span>
    <span>{staleReviews}</span>
  </button>
  <!-- Archived has no count: an item that comes to Need attention leaves it (#992). -->
  <button aria-current={current({ kind: "archived" })} onclick={() => onselect({ kind: "archived" })}>
    <span>Archived</span>
  </button>
  {#if filters.length > 0 || person}
    <h2 class="small-heading">Filters</h2>
  {/if}
  <!-- Two Filters may share a search, so a search can't be the key. -->
  {#each filters as filter, index (index)}
    <button
      aria-current={current({ kind: "filter", query: filter.query })}
      onclick={() => onselect({ kind: "filter", query: filter.query })}
    >
      <span>{filter.name}</span>
      <span>{filter.count}</span>
    </button>
    {#if filter.problem}
      <small>{filter.problem}</small>
    {/if}
  {/each}
  <!-- The person Filter a clicked name opened, marked unsaved, with Save and a way to close it (#472). -->
  {#if person}
    {@const login = person.login}
    {@const query = person.query}
    <div class="unsaved">
      <button
        class="person"
        aria-current={current({ kind: "person", login })}
        onclick={() => onopenperson(login)}
      >
        <span>{login}</span>
        <span class="mark">unsaved</span>
      </button>
      {#if query}
        <button
          class="act"
          aria-label="Save as Filter"
          title="Save as Filter"
          onclick={() => onsaveperson({ name: login, query })}>Save</button
        >
      {/if}
      <button class="act" aria-label="Close the unsaved Filter" title="Close" onclick={oncloseperson}
        >×</button
      >
    </div>
  {/if}
  <h2 class="small-heading">Pinned</h2>
  <button aria-current={current({ kind: "pinned" })} onclick={() => onselect({ kind: "pinned" })}>
    <span>Pinned issues</span>
    <span>{pinned}</span>
  </button>
</nav>

<style>
  /* A step off the content, with a border, as VS Code draws its side bar (#374). */
  nav {
    display: flex;
    flex: none;
    flex-direction: column;
    align-self: stretch;
    gap: 1px;
    /* The edge measures the whole box, padding and border too, and gives that width back. */
    box-sizing: border-box;
    width: var(--sidebar-width, 200px);
    padding: 6px;
    border-right: 1px solid var(--border);
    overflow-y: auto;
    overscroll-behavior: contain;
    font-size: 13px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
  }

  /* An entry: a line of text with its count at the right, tinted under the pointer. */
  button {
    all: unset;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
  }

  button:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  button:focus-visible {
    outline: 1px solid var(--accent);
  }

  button > span + span {
    color: var(--fg-muted);
  }

  [aria-current] {
    font-weight: 600;
    background: var(--selected);
  }

  [aria-current] > span + span {
    font-weight: 400;
  }

  /* The Repository's name, and ⚙ at the right. */
  .top {
    display: flex;
    gap: 2px;
    margin-bottom: 4px;
  }

  .repository {
    /* Not flex like the other lines: the ellipsis shows only on a block's own text. */
    display: block;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settings,
  .fold {
    flex: none;
    color: var(--fg-muted);
  }

  h2 {
    margin: 0;
    padding: 12px 8px 4px;
  }

  /* What went wrong with the Filter above. */
  small {
    padding: 0 8px 4px;
  }

  /* The unsaved person Filter's line, with Save and Close beside it (#472). */
  .unsaved {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .unsaved .person {
    flex: 1;
    min-width: 0;
  }

  /* The mark that says the Filter is unsaved, small and muted, as the count of a Filter sits. */
  .mark {
    font-size: 11px;
    font-weight: 400;
    color: var(--fg-muted);
  }

  .act {
    flex: none;
    padding: 2px 4px;
    font-size: 11px;
    color: var(--fg-muted);
  }
</style>
