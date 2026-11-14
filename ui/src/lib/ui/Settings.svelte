<!-- The Settings dialog (docs/devel/settings.md), laid out as variant B of the Settings prototype on
     `prototype/app-shell`: the pages in a column on the left, and the open one on the right. It lists only
     Account for now: #520 adds the other pages, switching between them, and Apply and Cancel. -->
<script lang="ts">
  import type { Status } from "$lib/state/status.svelte";
  import AccountPage from "./AccountPage.svelte";
  import { closeOnPress, takeFocus } from "./focus";

  let {
    status,
    onclose,
  }: {
    /** The app-wide status, which says who is signed in. */
    status: Status | undefined;
    onclose: () => void;
  } = $props();

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's, beside ✕. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="settings-scrim" onmousedown={closeOnPress(onclose)}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Settings"
    tabindex="-1"
    {@attach takeFocus}
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <header>
      <h2>Settings</h2>
      <button class="close" aria-label="Close" title="Close (Esc)" onclick={onclose}>✕</button>
    </header>
    <div class="body">
      <nav aria-label="Settings pages">
        <button aria-current="page">Account</button>
      </nav>
      <section aria-label="Account">
        <AccountPage {status} />
      </section>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
  }

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

  section {
    min-height: 0;
    padding: 6px 20px 20px;
    overflow: auto;
  }
</style>
