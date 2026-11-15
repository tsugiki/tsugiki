<!-- The Settings dialog (docs/devel/settings.md), laid out as variant B of the Settings prototype on
     `prototype/app-shell`: the pages in a column on the left, each Repository's under Repositories, the open
     one on the right, and Cancel and Apply under them. `SettingsDialog` decides what each does. It opens on
     General, or on Advanced at a language server's row, as the Definition peek's link asks (#683). -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { About } from "$lib/state/about.svelte";
  import type { ServerName } from "$lib/state/server-words";
  import type { SettingsState } from "$lib/state/settings.svelte";
  import {
    SETTINGS_PAGES,
    SettingsDialog,
    type SettingsPage,
    type Shown,
  } from "$lib/state/settings-dialog.svelte";
  import type { Status } from "$lib/state/status.svelte";
  import AccountPage from "./AccountPage.svelte";
  import AdvancedPage from "./AdvancedPage.svelte";
  import GeneralPage from "./GeneralPage.svelte";
  import { closeOnPress, keepFocusIn, takeFocus } from "./focus";

  let {
    settings,
    status,
    about,
    repository,
    at,
    onreread,
    onclose,
  }: {
    /** The settings in force, which the dialog previews its changes on. */
    settings: SettingsState;
    /** The app-wide status, which says who is signed in. */
    status: Status | undefined;
    /** What the core says about itself, which says where this build's source is. */
    about: About | undefined;
    /** The open Repository, whose language servers' logs Settings › Advanced opens, if one is open. */
    repository?: string | undefined;
    /** The language server whose row in Settings › Advanced the dialog opens at, or nothing to open on
     * General. */
    at?: ServerName | undefined;
    /** Runs once the dialog has read the files again, so the rest of the window takes what they say now,
     * such as a theme edited by hand. */
    onreread: () => void;
    onclose: () => void;
  } = $props();

  const dialog = new SettingsDialog(
    untrack(() => settings),
    untrack((): Shown => (at === undefined ? { page: "General" } : { page: "Advanced" })),
  );
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
  function cancelOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") cancel();
  }
</script>

<svelte:window onkeydown={cancelOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's, beside ✕. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="settings-scrim" onmousedown={closeOnPress(cancel)}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Settings"
    tabindex="-1"
    {@attach takeFocus}
    onkeydown={keepFocusIn}
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <header>
      <h2>Settings</h2>
      <button class="close" aria-label="Close" title="Close without applying (Esc)" onclick={cancel}>✕</button
      >
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
          <h3 class="repository mono">{dialog.shown.repository}</h3>
          <p class="empty">Nothing to set here yet.</p>
        {:else if dialog.shown.page === "General"}
          <GeneralPage {dialog} />
        {:else if dialog.shown.page === "Account"}
          <AccountPage {status} {about} />
        {:else if dialog.shown.page === "Advanced"}
          <AdvancedPage {dialog} {repository} {at} />
        {:else}
          <p class="empty">Nothing to set here yet.</p>
        {/if}
      </section>
    </div>
    <footer>
      {#if dialog.problem !== undefined}
        <p role="alert">{dialog.problem}</p>
      {/if}
      <button disabled={dialog.applying} onclick={cancel}>Cancel</button>
      <button class="primary" disabled={dialog.applying} onclick={() => void apply()}
        >{dialog.changed ? "Apply" : "OK"}</button
      >
    </footer>
  </div>
</div>

<style>
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(760px, calc(100vw - 32px));
    height: min(520px, calc(100vh - 80px));
    margin: 40px auto 0;
    box-sizing: border-box;
    overflow: hidden;
    font-size: 13px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 30px var(--shadow);
  }

  .dialog:focus {
    outline: none;
  }

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

  h3.repository {
    margin: 4px 0 8px;
    padding-bottom: 4px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
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
