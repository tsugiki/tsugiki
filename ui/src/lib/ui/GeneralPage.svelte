<!-- Settings › General (docs/devel/settings.md): the settings of the whole window, in groups. Each change
     shows on the window at once, and the dialog's Apply keeps it. -->
<script lang="ts">
  import type { Settings } from "$lib/state/settings.svelte";
  import type { SettingsDialog } from "$lib/state/settings-dialog.svelte";
  import StoredIn from "./StoredIn.svelte";

  let { dialog }: { dialog: SettingsDialog } = $props();

  /** What a checkbox's change does to the settings. */
  const toggle =
    (set: (settings: Settings, on: boolean) => void) => (e: Event & { currentTarget: HTMLInputElement }) => {
      const on = e.currentTarget.checked;
      dialog.change((settings) => {
        set(settings, on);
      });
    };
</script>

<h3>Status bar</h3>
<div class="row">
  <label>
    <input
      type="checkbox"
      checked={dialog.current?.status_bar.visible}
      disabled={dialog.current === undefined}
      onchange={toggle((settings, on) => {
        settings.status_bar.visible = on;
      })}
    />
    Show the status bar
  </label>
  <StoredIn where="settings" />
  <p class="help">Problems, and being offline, still show when it's off.</p>
</div>

<h3>Updates</h3>
<div class="row">
  <label>
    <input
      type="checkbox"
      checked={dialog.current?.releases.check}
      disabled={dialog.current === undefined}
      onchange={toggle((settings, on) => {
        settings.releases.check = on;
      })}
    />
    Check for new versions
  </label>
  <StoredIn where="settings" />
  <p class="help">
    Asks GitHub Releases for a newer Tsugiki at start-up and every 6 hours, without your token.
  </p>
  <p class="help">Takes effect at the next start</p>
</div>

<style>
  h3 {
    margin: 16px 0 8px;
    padding-bottom: 4px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }

  h3:first-child {
    margin-top: 4px;
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

  input {
    margin: 0;
  }

  .help {
    margin: 3px 0 0 22px;
    font-size: 12px;
    color: var(--fg-muted);
  }
</style>
