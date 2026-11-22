<!-- Settings › Repositories (docs/devel/settings.md, #533): each Repository on a line of its own, which opens
     its page, and Add repository… under them. Each time the page shows, it asks the core again what each
     Repository has on this machine, once the dialog has read the files. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { SettingsDialog } from "$lib/state/settings-dialog.svelte";
  import { overviewLine } from "./repository-overview";

  let { dialog, onadd }: { dialog: SettingsDialog; onadd: () => void } = $props();

  void untrack(() => dialog).measure();
</script>

<h3>Repositories</h3>
{#if dialog.listed.length > 0}
  <ul>
    <!-- By place: a name written twice in settings.toml lists twice. -->
    {#each dialog.listed as repository, at (at)}
      <li>
        <button
          onclick={() => {
            dialog.shown = { repository: repository.name };
          }}
        >
          <span class="name mono">{repository.name}</span>
          <span class="line">{overviewLine(repository, dialog.onThisMachine(repository.name))}</span>
          <span class="go" aria-hidden="true">›</span>
        </button>
      </li>
    {/each}
  </ul>
{/if}
{#if dialog.machineProblem !== undefined}
  <p class="problem">{dialog.machineProblem}</p>
{/if}
<!-- Off until the dialog has read the files, as its other controls are, so Cancel has them to keep the
     Repository it adds in, and while Apply saves, whose answer replaces the settings. -->
<button class="add" disabled={dialog.current === undefined || dialog.applying} onclick={onadd}
  >Add repository…</button
>

<style>
  ul {
    margin: 0 0 10px;
    padding: 0;
    overflow: hidden;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  li + li {
    border-top: 1px solid var(--border);
  }

  li button {
    all: unset;
    display: grid;
    grid-template-columns: 1fr auto;
    box-sizing: border-box;
    width: 100%;
    padding: 8px 12px;
    cursor: pointer;
  }

  li button:hover {
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  li button:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  .name,
  .line {
    grid-column: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .go {
    grid-row: 1 / 3;
    grid-column: 2;
    align-self: center;
    font-size: 18px;
    color: var(--fg-muted);
  }

  .problem {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--danger);
  }
</style>
