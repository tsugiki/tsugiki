<!-- The rail's Search tab (#686): a box for a query over the PR's code, with toggles for match case, whole
     word and regex, a line that says what it found, and the results grouped by file. The list draws only the
     rows on screen. ↓ in the box goes to the list, where ↑ ↓ move, ← → fold and unfold a file, and Enter or
     a click opens a match. The PR's diff is read once the tab shows, as each query names its ends. -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { PrViewState } from "$lib/state/pr-view.svelte";
  import { type LineHit, type SearchOption, searchDiff } from "$lib/state/text-search.svelte";
  import { currentOs, keyLabel, keyName } from "./keys";

  let { pr }: { pr: PrViewState } = $props();

  /** A row's height in pixels. */
  const ROW = 22;
  /** The rows drawn past each end of what shows. */
  const OVERSCAN = 10;
  const os = currentOs();
  const id = $props.id();

  const search = $derived(pr.search);
  /** What the search can run on: the ends of the diff the Files tab shows, or why there are none yet. */
  const diff = $derived(searchDiff(pr.files.view, pr.files.error));

  let list = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let height = $state(0);

  // Each runs again for another PR, another diff, and a new list or query, and for nothing the call itself
  // reads.
  $effect(() => {
    const view = pr;
    untrack(() => {
      view.loadFiles();
    });
  });
  $effect(() => {
    const [query, on] = [search, diff];
    untrack(() => {
      query.follow(on);
    });
  });
  // A list drawn anew, or the results of a new query, start at the top.
  $effect(() => {
    const [scroller] = [list, search.answered];
    untrack(() => {
      if (scroller !== null) scroller.scrollTop = 0;
      scrollTop = 0;
    });
  });
  const rows = $derived(search.rows);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.ceil((scrollTop + height) / ROW) + OVERSCAN);

  /** The toggles beside the box, each with its key while the box has the focus. */
  const OPTIONS: { option: SearchOption; label: string; title: string; key: string }[] = [
    { option: "case", label: "Aa", title: "Match Case", key: "alt+c" },
    { option: "words", label: "ab", title: "Match Whole Word", key: "alt+w" },
    { option: "regex", label: ".*", title: "Use Regular Expression", key: "alt+r" },
  ];
  const pressed: Record<SearchOption, () => boolean> = {
    case: () => search.matchCase,
    words: () => search.wholeWord,
    regex: () => search.regex,
  };

  /** Gives the box the focus with its text selected, as `Ctrl+Shift+F` asks, once it shows, and again on
   * each press. */
  function focused(box: HTMLInputElement) {
    if (!search.wantsFocus) return;
    search.wantsFocus = false;
    box.focus();
    box.select();
  }

  /** The box's keys: a toggle's, and ↓, which goes to the list. */
  function onboxkeydown(e: KeyboardEvent) {
    const key = keyName(e, os);
    const option = OPTIONS.find((each) => each.key === key)?.option;
    if (option !== undefined) {
      e.preventDefault();
      e.stopPropagation();
      search.toggle(option);
    } else if (key === "arrowdown" && rows.length > 0) {
      e.preventDefault();
      e.stopPropagation();
      list?.focus();
    }
  }

  /** Scrolls `scroller`, the list, as little as it takes to show the row under the cursor. */
  function showCursor(scroller: HTMLElement) {
    const top = search.cursor * ROW;
    scroller.scrollTop = Math.min(top, Math.max(scroller.scrollTop, top + ROW - scroller.clientHeight));
    scrollTop = scroller.scrollTop;
  }

  /** The list's keys, which go no further, so the Issue list's cursor stays where it is. */
  async function onlistkeydown(e: KeyboardEvent & { currentTarget: HTMLElement }) {
    const key = keyName(e, os);
    const moves: Record<string, () => void> = {
      arrowdown: () => {
        search.moveTo(search.cursor + 1);
      },
      arrowup: () => {
        search.moveTo(search.cursor - 1);
      },
      home: () => {
        search.moveTo(0);
      },
      end: () => {
        search.moveTo(rows.length - 1);
      },
      pagedown: () => {
        search.moveTo(search.cursor + pageRows());
      },
      pageup: () => {
        search.moveTo(search.cursor - pageRows());
      },
      arrowleft: () => {
        search.fold();
      },
      arrowright: () => {
        search.unfold();
      },
      enter: () => {
        const match = search.pick();
        if (match !== undefined) void pr.openMatch(match);
      },
    };
    const move = moves[key ?? ""];
    if (move === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    const scroller = e.currentTarget;
    move();
    await tick();
    showCursor(scroller);
  }

  const pageRows = () => Math.max(1, Math.floor(height / ROW) - 1);

  /** A click on a row puts the cursor on it, and opens its match or flips its file's fold. */
  function onrowclick(index: number) {
    search.moveTo(index);
    const match = search.pick();
    if (match !== undefined) void pr.openMatch(match);
  }

  /** A line's text in pieces, each marked if it is a match, with an ellipsis where a long line was cut. */
  function pieces(hit: LineHit): { text: string; match: boolean }[] {
    const out: { text: string; match: boolean }[] = [];
    let at = 0;
    for (const range of hit.ranges) {
      if (range.start > at) out.push({ text: hit.text.slice(at, range.start), match: false });
      out.push({ text: hit.text.slice(range.start, range.end), match: true });
      at = range.end;
    }
    if (at < hit.text.length) out.push({ text: hit.text.slice(at), match: false });
    const before = hit.cut === "start" || hit.cut === "both" ? "…" : "";
    const after = hit.cut === "end" || hit.cut === "both" ? "…" : "";
    return [{ text: before, match: false }, ...out, { text: after, match: false }].filter(
      (piece) => piece.text !== "",
    );
  }

  /** The id of row `index`, which the list names as the row under the cursor. */
  const rowId = (index: number) => `${id}-row-${String(index)}`;
</script>

<div class="search">
  <div class="query">
    <input
      type="text"
      aria-label="Search the PR's code"
      placeholder="Search"
      spellcheck="false"
      value={search.text}
      {@attach focused}
      oninput={(e) => {
        search.type(e.currentTarget.value);
      }}
      onkeydown={onboxkeydown}
    />
    {#each OPTIONS as { option, label, title, key } (option)}
      <button
        type="button"
        class={["option", option, option === "regex" && "mono"]}
        aria-label={title}
        aria-pressed={pressed[option]()}
        title={`${title} (${keyLabel(key, os)})`}
        onclick={() => {
          search.toggle(option);
        }}>{label}</button
      >
    {/each}
  </div>
  <p class={["summary", search.troubled && "troubled"]} aria-live="polite">{search.summary}</p>
  {#if rows.length > 0}
    <div
      class="results"
      role="tree"
      aria-label="Search results"
      tabindex="0"
      aria-activedescendant={search.cursor >= first && search.cursor < last
        ? rowId(search.cursor)
        : undefined}
      bind:this={list}
      bind:clientHeight={height}
      onscroll={(e) => {
        scrollTop = e.currentTarget.scrollTop;
      }}
      onkeydown={(e) => void onlistkeydown(e)}
    >
      <div class="spacer" style:height={`${String(rows.length * ROW)}px`}>
        {#each rows.slice(first, last) as row, at (first + at)}
          {@const index = first + at}
          <!-- The list holds the focus, and names the row under the cursor, so a row takes none. -->
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
          <div
            id={rowId(index)}
            class={["row", row.kind, row.file.generated && "generated", index === search.cursor && "cursor"]}
            role="treeitem"
            aria-level={row.kind === "file" ? 1 : 2}
            aria-selected={index === search.cursor}
            aria-expanded={row.kind === "file" ? !row.folded : undefined}
            style:top={`${String(index * ROW)}px`}
            onclick={() => {
              onrowclick(index);
            }}
          >
            {#if row.kind === "file"}
              <span class="chevron" aria-hidden="true">{row.folded ? "▸" : "▾"}</span>
              <span class="path" title={row.file.path}>{row.file.path}</span>
              {#if row.file.generated}<span class="tag">Generated</span>{/if}
              <span class="count">{row.matches}</span>
            {:else}
              <span class="number">{row.hit.line}</span>
              <!-- prettier-ignore -->
              <span class="text mono">{#each pieces(row.hit) as piece, at (at)}{#if piece.match}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}</span>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  /* Fills the rail's pane: the box and the line stay put, and the results scroll under them. */
  .search {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .query {
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 0 8px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
  }

  .query:focus-within {
    border-color: var(--focus-border);
  }

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    padding: 2px 4px;
    font: inherit;
    color: var(--fg);
    background: transparent;
  }

  .option {
    all: unset;
    cursor: pointer;
    padding: 0 4px;
    font-size: 12px;
    line-height: 18px;
    border: 1px solid transparent;
    border-radius: 3px;
  }

  .option.words {
    text-decoration: underline;
  }

  .option:hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .option[aria-pressed="true"] {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .option:focus-visible {
    outline: 1px solid var(--focus-border);
  }

  .summary {
    min-height: 18px;
    margin: 4px 8px;
    font-size: 12px;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .summary.troubled {
    color: var(--danger);
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .results:focus-visible {
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  .spacer {
    position: relative;
  }

  .row {
    position: absolute;
    right: 0;
    left: 0;
    display: flex;
    gap: 6px;
    align-items: center;
    box-sizing: border-box;
    height: 22px;
    padding: 0 8px;
    white-space: nowrap;
    cursor: pointer;
  }

  .row:hover {
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  .row.cursor {
    background: var(--selected);
  }

  .results:focus .row.cursor {
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  .row.line {
    padding-left: 26px;
  }

  /* A Generated file's rows greyed, as the Files tab greys it. */
  .row.generated {
    color: var(--fg-muted);
  }

  .chevron {
    flex: none;
    width: 12px;
  }

  .path,
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .path {
    flex: 1;
  }

  .tag,
  .count,
  .number {
    flex: none;
    font-size: 11px;
    color: var(--fg-muted);
  }

  .count {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .number {
    min-width: 3ch;
    text-align: right;
  }

  .text {
    flex: 1;
  }

  mark {
    color: inherit;
    background: color-mix(in srgb, var(--find) 40%, transparent);
  }

  /* Windows' contrast themes drop the tints, so a match and a pressed toggle keep a border. */
  @media (forced-colors: active) {
    mark {
      outline: 1px solid Highlight;
    }

    .option[aria-pressed="true"] {
      border-color: Highlight;
    }
  }
</style>
