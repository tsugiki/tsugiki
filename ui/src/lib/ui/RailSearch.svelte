<!-- The rail's Search tab (#686): a box for a query over the PR's code, with toggles for match case, whole
     word and regex, a line that says what it found, and the results grouped by file. ↓ in the box goes to
     the list, where ← → fold and unfold a file, and Enter or a click opens a match. The PR's diff is read
     once the tab shows, as each query names its ends. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { PrViewState } from "$lib/state/pr-view.svelte";
  import { type LineHit, type SearchOption, type SearchRow, searchDiff } from "$lib/state/text-search.svelte";
  import { currentOs, keyLabel, keyName } from "./keys";
  import RailList, { type RailRow } from "./RailList.svelte";

  let { pr }: { pr: PrViewState } = $props();

  const os = currentOs();

  const search = $derived(pr.search);
  /** What the search can run on: the ends of the diff the Files tab shows, or why there are none yet. */
  const diff = $derived(searchDiff(pr.files));

  let list = $state<RailList>();

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
  const rows = $derived(search.rows);

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

  /** Opens the match under the cursor, or flips its file's fold. */
  function pick() {
    const match = search.pick();
    if (match !== undefined) void pr.openMatch(match);
  }

  /** How row `index` shows: a file open or folded, and a Generated file's rows greyed. */
  function shows(index: number): RailRow {
    const row = rows[index] as SearchRow;
    const generated = row.file.generated && "generated";
    if (row.kind === "file") return { level: 1, expanded: !row.folded, class: ["file", generated] };
    return { level: 2, class: ["line", generated] };
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
</script>

{#snippet drawn(index: number)}
  {@const row = rows[index] as SearchRow}
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
{/snippet}

<div class="search">
  <div class="rail-box">
    <input
      type="text"
      aria-label="Search the PR's code"
      onfocus={() => {
        search.boxFocused = true;
      }}
      onblur={() => {
        search.boxFocused = false;
      }}
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
    <RailList
      bind:this={list}
      label="Search results"
      count={rows.length}
      cursor={search.cursor}
      indent={18}
      {shows}
      row={drawn}
      top={search.answered}
      onmove={(index) => {
        search.moveTo(index);
      }}
      onfold={() => {
        search.fold();
      }}
      onunfold={() => {
        search.unfold();
      }}
      onpick={pick}
    />
  {/if}
</div>

<style>
  /* Fills the rail's pane: the box and the line stay put, and the results scroll under them. */
  .search {
    display: flex;
    flex-direction: column;
    height: 100%;
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
