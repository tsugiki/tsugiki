<!-- A Repository's page in Settings (docs/devel/settings.md, #534). Its header names the Repository and says
     whether Tsugiki can write there, with why not, its open Issues and PRs as GitHub last counted them, and
     its last sync, which moves on while the page shows. Each part waits for the core's answer, and the line
     shows once one has come. Its first row switches Read-only (#915). Its Filters follow, which the user
     renames, edits, moves, deletes and adds. The dialog's Apply saves both (#535). -->
<script lang="ts">
  import { isBlank } from "$lib/state/filter-edits.svelte";
  import { RepositoryStateView } from "$lib/state/repository-state.svelte";
  import { RepositorySyncView } from "$lib/state/repository-sync.svelte";
  import type { SettingsDialog } from "$lib/state/settings-dialog.svelte";
  import { minuteClock } from "./clock.svelte";
  import { openWords, writeWords } from "./repository-header";
  import StoredIn from "./StoredIn.svelte";
  import { syncedWhen, syncedWords } from "./synced";

  let { dialog, repository }: { dialog: SettingsDialog; repository: string } = $props();

  // Another Repository's page asks about that one, so a switch starts new views and stops the last.
  let stateView = $state<RepositoryStateView>();
  let syncView = $state<RepositorySyncView>();
  $effect(() => {
    const views = { state: new RepositoryStateView(repository), sync: new RepositorySyncView(repository) };
    stateView = views.state;
    syncView = views.sync;
    void views.state.load();
    void views.sync.load();
    return () => {
      views.state.stop();
      views.sync.stop();
    };
  });

  // "Synced 2 min ago" has to move on by itself between syncs.
  const now = minuteClock();
  const sync = $derived(syncView?.state);
  /** One part of the header line, with the tooltip its own time gives it, if it has one. */
  const plain = (words: string | null) =>
    words === null ? null : { words, title: undefined as string | undefined };
  const parts = $derived(
    [
      plain(writeWords(stateView?.state, repository)),
      plain(sync?.open ? openWords(sync.open) : null),
      sync === undefined
        ? null
        : { words: syncedWords(sync.last_sync, now), title: syncedWhen(sync.last_sync) },
    ].filter((part) => part !== null),
  );

  /** Whether this page has Read-only on, once the dialog has read the files (#915). */
  const readOnly = $derived(dialog.readOnly(repository));
  /** The Filters of this page, once the dialog has read the files. */
  const filters = $derived(dialog.filters.page(repository));
  /** Whether the next name field to show is the one Add filter added, which takes the focus. */
  let added = false;
  /** Gives the name field of the Filter Add filter added the focus. */
  const focusAdded = (input: HTMLInputElement) => {
    if (added) {
      added = false;
      input.focus();
    }
  };
</script>

<h3 class="mono">{repository}</h3>
{#if parts.length > 0}
  <p class="facts">
    {#each parts as part, index (index)}{#if index > 0}{" · "}{/if}<span title={part.title}>{part.words}</span
      >{/each}
  </p>
{/if}

<div class="row">
  <label>
    <input
      type="checkbox"
      checked={readOnly}
      disabled={readOnly === undefined || dialog.applying}
      onchange={(e) => {
        dialog.chooseReadOnly(repository, e.currentTarget.checked);
      }}
    />
    Read-only
  </label>
  <StoredIn where="settings" />
  <p class="help">Tsugiki sends nothing to GitHub here, such as a comment or a review.</p>
</div>

<h3>Filters <StoredIn where="settings" /></h3>
<p class="help">
  The Sidebar lists them in this order. Each searches this Repository in GitHub's search syntax.
</p>
{#if filters !== undefined}
  {@const page = filters}
  <div class="filters">
    {#each page.rows as row, at (row.id)}
      <div class={["filter", { blank: isBlank(row) }]} role="group" aria-label={`Filter ${at + 1}`}>
        <input
          aria-label="Name"
          placeholder="Name"
          aria-invalid={row.name.trim() === ""}
          disabled={dialog.applying}
          bind:value={row.name}
          {@attach focusAdded}
        />
        <input
          class="mono"
          aria-label="Search"
          placeholder="Search, such as is:open label:bug"
          spellcheck="false"
          aria-invalid={row.query.trim() === ""}
          disabled={dialog.applying}
          bind:value={row.query}
        />
        <button
          aria-label="Move up"
          title="Move up"
          disabled={dialog.applying || at === 0}
          onclick={() => {
            page.move(at, -1);
          }}>↑</button
        >
        <button
          aria-label="Move down"
          title="Move down"
          disabled={dialog.applying || at === page.rows.length - 1}
          onclick={() => {
            page.move(at, 1);
          }}>↓</button
        >
        <button
          disabled={dialog.applying}
          onclick={() => {
            page.delete(at);
          }}>Delete</button
        >
      </div>
    {/each}
  </div>
  <button
    disabled={dialog.applying}
    onclick={() => {
      added = true;
      page.add();
    }}>Add filter</button
  >
{/if}

<style>
  .facts {
    margin: 0 0 12px;
    color: var(--fg-muted);
  }

  h3 {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .row {
    margin-bottom: 10px;
  }

  label {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
  }

  label input {
    margin: 0;
  }

  .help {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .row .help {
    margin: 3px 0 0 22px;
  }

  .filters {
    margin: 0 0 8px;
  }

  .filter {
    display: grid;
    grid-template-columns: 160px minmax(0, 1fr) auto auto auto;
    gap: 6px;
    align-items: center;
    margin-bottom: 4px;
    padding-left: 4px;
    border-left: 2px solid transparent;
  }

  /* A Filter with a blank name or search, which Apply can't save. */
  .filter.blank {
    border-left-color: var(--danger);
  }

  input {
    min-width: 0;
  }

  input[aria-invalid="true"] {
    outline: 1px solid var(--danger);
  }
</style>
