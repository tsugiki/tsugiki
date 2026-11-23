<!-- The Settings dialog (docs/devel/settings.md), laid out as variant B of the Settings prototype on
     `prototype/app-shell`: the pages in a column on the left, each Repository's under Repositories, the open
     one on the right, and Cancel and Apply under them. `SettingsDialog` decides what each does. It opens on
     General, or on Advanced at a language server's row, as the Definition peek's link asks (#683), or on a
     Repository's page, as the link in a Read-only Repository's line asks (#915). Settings › Repositories
     opens Add repository… over it (#533). -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { About } from "$lib/state/about.svelte";
  import type { Home } from "$lib/state/home.svelte";
  import type { SettingsAt } from "$lib/state/settings-ask.svelte";
  import type { SettingsState } from "$lib/state/settings.svelte";
  import {
    SETTINGS_PAGES,
    SettingsDialog,
    type SettingsPage,
    type Shown,
  } from "$lib/state/settings-dialog.svelte";
  import type { Status } from "$lib/state/status.svelte";
  import type { ThemeState } from "$lib/state/theme.svelte";
  import AccountPage from "./AccountPage.svelte";
  import AddRepository from "./AddRepository.svelte";
  import AdvancedPage from "./AdvancedPage.svelte";
  import GeneralPage from "./GeneralPage.svelte";
  import KeyboardPage from "./KeyboardPage.svelte";
  import Dialog from "./Dialog.svelte";
  import RepositoriesPage from "./RepositoriesPage.svelte";
  import RepositoryPage from "./RepositoryPage.svelte";

  let {
    settings,
    theme,
    status,
    about,
    repository,
    at,
    onreread,
    onadded,
    onswitch,
    onclose,
  }: {
    /** The settings in force, which the dialog previews its changes on. */
    settings: SettingsState;
    /** The window's themes, which Settings › General lists and previews `[appearance]` on. */
    theme: ThemeState;
    /** The app-wide status, which says who is signed in. */
    status: Status | undefined;
    /** What the core says about itself, which says where this build's source is. */
    about: About | undefined;
    /** The open Repository, whose language servers' logs Settings › Advanced opens, if one is open. */
    repository?: string | undefined;
    /** The language server's row in Settings › Advanced or the Repository's page the dialog opens at, or
     * nothing to open on General. */
    at?: SettingsAt | undefined;
    /** Runs once the dialog has read the files again, so the rest of the window takes what they say now,
     * such as a theme edited by hand. */
    onreread: () => void;
    /** Runs once Add repository…, on Settings › Repositories, has added a Repository and the core has opened
     * it, with what the window shows now. */
    onadded: (home: Home) => void;
    /** Runs when the user asks to switch to `name`, a Repository Tsugiki has already, from Add
     * repository…, once the dialog has closed as Cancel closes it. */
    onswitch: (name: string) => void;
    onclose: () => void;
  } = $props();

  /** Whether Add repository… is open over the dialog. */
  let adding = $state(false);

  const dialog = new SettingsDialog(
    untrack(() => settings),
    untrack(() => theme),
    untrack((): Shown => {
      if (at === undefined) return { page: "General" };
      return "server" in at ? { page: "Advanced" } : { repository: at.repository };
    }),
  );
  /** The language server whose row in Settings › Advanced the dialog opened at, if it did. */
  const server = $derived(at !== undefined && "server" in at ? at.server : undefined);
  void dialog.open().then((reread) => {
    if (reread) onreread();
  });

  /** The name of what the right side shows, for its region. */
  const title = $derived("repository" in dialog.shown ? dialog.shown.repository : dialog.shown.page);
  /** Whether the right side shows `page`, for its entry's `aria-current`. */
  const pageShown = (page: SettingsPage) =>
    "page" in dialog.shown && dialog.shown.page === page ? "page" : undefined;
  /** Whether the right side shows `repository`'s page, for its entry's `aria-current`. */
  const repositoryShown = (repository: string) =>
    "repository" in dialog.shown && dialog.shown.repository === repository ? "page" : undefined;
  /** A Repository's name without its owner, which its title gives in full. */
  const short = (repository: string) => repository.slice(repository.indexOf("/") + 1);

  /** Cancel, Esc, ✕ and a press outside put the settings back and close, unless Apply is saving. */
  function cancel() {
    if (dialog.cancel()) onclose();
  }
  async function apply() {
    if (await dialog.apply()) onclose();
  }
</script>

<Dialog label="Settings" testid="settings" size="large" flush onclose={cancel}>
  <header>
    <h2>Settings</h2>
    <button class="close" aria-label="Close" title="Close without applying (Esc)" onclick={cancel}>✕</button>
  </header>
  <div class="body">
    <nav aria-label="Settings pages">
      {#each SETTINGS_PAGES as page (page)}
        <button
          aria-current={pageShown(page)}
          onclick={() => {
            dialog.shown = { page };
          }}>{page}</button
        >
        {#if page === "Repositories"}
          <!-- By place: a name written twice in settings.toml lists twice. -->
          {#each dialog.repositories as repository, at (at)}
            <button
              class="repository"
              aria-label={repository}
              title={repository}
              aria-current={repositoryShown(repository)}
              onclick={() => {
                dialog.shown = { repository };
              }}><span class="mono">{short(repository)}</span></button
            >
          {/each}
        {/if}
      {/each}
    </nav>
    <section aria-label={title}>
      {#if "repository" in dialog.shown}
        <RepositoryPage {dialog} repository={dialog.shown.repository} />
      {:else if dialog.shown.page === "General"}
        <GeneralPage {dialog} />
      {:else if dialog.shown.page === "Repositories"}
        <RepositoriesPage
          {dialog}
          onadd={() => {
            adding = true;
          }}
        />
      {:else if dialog.shown.page === "Account"}
        <AccountPage {status} {about} />
      {:else if dialog.shown.page === "Keyboard"}
        <KeyboardPage />
      {:else if dialog.shown.page === "Advanced"}
        <AdvancedPage {dialog} {repository} at={server} />
      {:else}
        <p class="empty">Nothing to set here yet.</p>
      {/if}
    </section>
  </div>
  <footer>
    <!-- The reason Apply is off comes first: it holds now, and a problem from the last try waits behind
         it. A screen reader says the reason when it comes, as it says the problem. -->
    {#if dialog.unsaveable !== undefined}
      <p role="status">{dialog.unsaveable}</p>
    {:else if dialog.problem !== undefined}
      <p role="alert">{dialog.problem}</p>
    {/if}
    <button disabled={dialog.applying} onclick={cancel}>Cancel</button>
    <button
      class="primary"
      disabled={dialog.applying || dialog.unsaveable !== undefined}
      onclick={() => void apply()}>{dialog.changed ? "Apply" : "OK"}</button
    >
  </footer>
</Dialog>
{#if adding}
  <AddRepository
    onclose={() => {
      adding = false;
    }}
    onadded={(home) => {
      adding = false;
      dialog.added(home);
      onadded(home);
    }}
    onswitch={(name) => {
      // The user asked to go to that Repository, so the dialog goes too, as Cancel takes it. While Apply
      // saves, the dialog stays and the window stays where it is.
      adding = false;
      if (dialog.cancel()) {
        onclose();
        onswitch(name);
      }
    }}
  />
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  .close {
    all: unset;
    padding: 0 6px;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .close:focus-visible {
    outline: 1px solid var(--accent);
  }

  .body {
    display: grid;
    flex: 1;
    grid-template-columns: 170px 1fr;
    min-height: 0;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    overflow: auto;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
    border-right: 1px solid var(--border);
  }

  nav button {
    all: unset;
    padding: 6px 10px;
    border-radius: 4px;
    cursor: pointer;
  }

  nav button:focus-visible {
    outline: 1px solid var(--accent);
  }

  nav [aria-current] {
    font-weight: 600;
    background: var(--selected);
  }

  nav .repository {
    padding: 4px 10px 4px 22px;
    overflow: hidden;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  section {
    min-height: 0;
    padding: 6px 20px 20px;
    overflow: auto;
  }

  /* A heading over a group of settings, on each page and over a Repository's. */
  section :global(h3) {
    margin: 16px 0 8px;
    padding-bottom: 4px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }

  section :global(h3:first-child) {
    margin-top: 4px;
  }

  .empty {
    color: var(--fg-muted);
  }

  footer {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: flex-end;
    padding: 10px 14px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
    border-top: 1px solid var(--border);
  }

  footer p {
    flex: 1;
    margin: 0;
    color: var(--danger);
  }
</style>
