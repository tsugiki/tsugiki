<!-- PROTOTYPE, throwaway (#374): the floating bar that switches the palette (Ctrl+Shift+← / →) and the mode,
     System, Light or Dark (Ctrl+Shift+↑ / ↓). A mode other than System calls Tauri's `setTheme`, which on
     Windows also sets WebView2's preferred colour scheme, so the title bar and `prefers-color-scheme` should
     follow; the readout shows whether they did. Shown only in a development build. -->
<script lang="ts">
  import "./themes.css";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  type Mode = "system" | "light" | "dark";
  const PALETTES = [
    { key: "A", name: "GitHub Primer" },
    { key: "B", name: "VS Code Modern" },
    { key: "C", name: "Browser defaults" },
  ] as const;
  const MODES: Mode[] = ["system", "light", "dark"];

  let palette = $state<string>("A");
  let mode = $state<Mode>("system");
  let fade = $state(false);
  let media = $state<"light" | "dark">("light");
  let windowTheme = $state<string>("?");
  let note = $state("");

  const index = $derived(PALETTES.findIndex((one) => one.key === palette));
  const current = $derived(PALETTES[index]);

  const fadeOnce = () => {
    if (!fade) return;
    const root = document.documentElement;
    root.classList.add("theme-fading");
    setTimeout(() => root.classList.remove("theme-fading"), 260);
  };

  const apply = () => {
    const root = document.documentElement;
    fadeOnce();
    root.dataset.palette = palette;
    root.style.colorScheme = mode === "system" ? "light dark" : mode;
    getCurrentWindow()
      .setTheme(mode === "system" ? null : mode)
      .then(() => (note = ""))
      .catch((e: unknown) => (note = `setTheme: ${String(e)}`));
  };

  const stepPalette = (by: number) => {
    const next = PALETTES[(index + by + PALETTES.length) % PALETTES.length];
    if (next) palette = next.key;
    apply();
  };
  const stepMode = (by: number) => {
    const at = MODES.indexOf(mode);
    mode = MODES[(at + by + MODES.length) % MODES.length] ?? "system";
    apply();
  };

  const onkeydown = (e: KeyboardEvent) => {
    if (!e.ctrlKey || !e.shiftKey || e.altKey) return;
    const by = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -1, ArrowDown: 1 }[e.key];
    if (by === undefined) return;
    e.preventDefault();
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") stepPalette(by);
    else stepMode(by);
  };

  onMount(() => {
    apply();
    const query = matchMedia("(prefers-color-scheme: dark)");
    const read = () => (media = query.matches ? "dark" : "light");
    read();
    query.addEventListener("change", read);
    let unlisten: (() => void) | undefined;
    try {
      const win = getCurrentWindow();
      void win.theme().then((t) => (windowTheme = t ?? "none"));
      void win
        .onThemeChanged(({ payload }) => {
          windowTheme = payload;
          fadeOnce();
        })
        .then((u) => (unlisten = u));
    } catch (e) {
      note = `no Tauri window: ${String(e)}`;
    }
    return () => {
      query.removeEventListener("change", read);
      unlisten?.();
    };
  });
</script>

<svelte:window {onkeydown} />

{#if import.meta.env.DEV}
  <div class="switcher" role="toolbar" aria-label="Prototype themes">
    <button type="button" aria-label="Previous palette" onclick={() => stepPalette(-1)}>‹</button>
    <span class="label">{current?.key} — {current?.name}</span>
    <button type="button" aria-label="Next palette" onclick={() => stepPalette(1)}>›</button>
    <label>
      Theme
      <select
        value={mode}
        onchange={(e) => {
          mode = e.currentTarget.value as Mode;
          apply();
        }}
      >
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
    <label><input type="checkbox" bind:checked={fade} /> Fade</label>
    <span class="readout" title="prefers-color-scheme in the webview, and the native window's theme">
      media: {media} · window: {windowTheme}
    </span>
    {#if note}<span class="note" title={note}>{note}</span>{/if}
  </div>
{/if}

<style>
  .switcher {
    position: fixed;
    bottom: 34px;
    left: 50%;
    z-index: 100;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 90vw;
    padding: 4px 10px;
    border-radius: 999px;
    transform: translateX(-50%);
    font-family: system-ui, sans-serif;
    font-size: 12px;
    color: #fff;
    background: #8250df;
    box-shadow: 0 4px 14px rgb(0 0 0 / 35%);
    color-scheme: dark;
  }

  button {
    width: 22px;
    border: 0;
    border-radius: 50%;
    font: inherit;
    font-size: 16px;
    color: inherit;
    background: rgb(255 255 255 / 15%);
    cursor: pointer;
  }

  .label {
    min-width: 150px;
    text-align: center;
  }

  select {
    font: inherit;
  }

  .readout {
    opacity: 0.85;
    font-variant-numeric: tabular-nums;
  }

  .note {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
