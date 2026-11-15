<!-- Settings › General (docs/devel/settings.md): the settings of the whole window, in groups. Each change
     shows on the window at once, and the dialog's Apply keeps it. -->
<script lang="ts">
  import type { Settings } from "$lib/state/settings.svelte";
  import type { SettingsDialog } from "$lib/state/settings-dialog.svelte";
  import type { AppearanceSettings, ThemeKind } from "$lib/state/theme.svelte";
  import StoredIn from "./StoredIn.svelte";

  let { dialog }: { dialog: SettingsDialog } = $props();

  const SIDE_KEYS = { light: "light_theme", dark: "dark_theme" } as const satisfies Record<
    ThemeKind,
    keyof AppearanceSettings
  >;

  const appearance = $derived(dialog.current?.appearance);
  /** The themes of each side that can be used, as the theme picker lists them, once the window has them.
   * The files that can't be used aren't offered. */
  const names = $derived({ light: dialog.themeNames("light"), dark: dialog.themeNames("dark") });
  /** Whether the Appearance rows wait: for the files, or for the window's themes. */
  const waiting = $derived(appearance === undefined || names.light === undefined || names.dark === undefined);
  /** Whether `theme` is one of the Theme row's own choices, so it needs no row of its own. */
  const offered = (theme: string) =>
    theme === "system" || !!names.light?.includes(theme) || !!names.dark?.includes(theme);
  /** The words for a `theme` the file holds that the row doesn't offer: a mode from a file written before,
   * which shows that side's theme, or a name no Theme has. Apply over another change leaves it in the
   * file. */
  const heldTheme = (theme: string) =>
    theme === "light"
      ? "Always the Light theme"
      : theme === "dark"
        ? "Always the Dark theme"
        : notFound(theme);
  /** The words for a name no Theme has, once the window has its themes to say so. */
  const notFound = (name: string) => (waiting ? name : `${name} (not found)`);

  /** What a select's change does to `[appearance]`. */
  const pick =
    (set: (appearance: AppearanceSettings, value: string) => void) =>
    (e: Event & { currentTarget: HTMLSelectElement }) => {
      const value = e.currentTarget.value;
      dialog.changeAppearance((appearance) => {
        set(appearance, value);
      });
    };

  /** What a checkbox's change does to the settings. */
  const toggle =
    (set: (settings: Settings, on: boolean) => void) => (e: Event & { currentTarget: HTMLInputElement }) => {
      const on = e.currentTarget.checked;
      dialog.change((settings) => {
        set(settings, on);
      });
    };
</script>

{#snippet sideRow(side: ThemeKind, title: string)}
  {@const key = SIDE_KEYS[side]}
  {@const held = appearance?.[key]}
  <div class="row">
    <label class="field" for="settings-{key}">{title}</label>
    <select
      id="settings-{key}"
      value={held}
      disabled={waiting}
      onchange={pick((appearance, name) => {
        appearance[key] = name;
      })}
    >
      {#if held !== undefined && !names[side]?.includes(held)}
        <option value={held}>{notFound(held)}</option>
      {/if}
      {#each names[side] ?? [] as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </select>
    <StoredIn where="settings" />
    <p class="help field-help">Follow system shows it while the system is {side}.</p>
  </div>
{/snippet}

<h3>Appearance</h3>
<div class="row">
  <label class="field" for="settings-theme">Theme</label>
  <select
    id="settings-theme"
    value={appearance?.theme}
    disabled={waiting}
    onchange={pick((appearance, theme) => {
      appearance.theme = theme;
    })}
  >
    {#if appearance !== undefined && !offered(appearance.theme)}
      <option value={appearance.theme}>{heldTheme(appearance.theme)}</option>
    {/if}
    <option value="system">Follow system</option>
    <optgroup label="Light">
      {#each names.light ?? [] as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </optgroup>
    <optgroup label="Dark">
      {#each names.dark ?? [] as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </optgroup>
  </select>
  <StoredIn where="settings" />
</div>
{@render sideRow("light", "Light theme")}
{@render sideRow("dark", "Dark theme")}

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

  label.field {
    display: inline-block;
    width: 96px;
    cursor: default;
  }

  select {
    margin-right: 6px;
  }

  .help {
    margin: 3px 0 0 22px;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .help.field-help {
    margin-left: 96px;
  }
</style>
