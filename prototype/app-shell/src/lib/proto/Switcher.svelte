<script lang="ts">
  // PROTOTYPE — throwaway. Floating variant switcher. Not gated on DEV: this whole app is a prototype
  // that only exists on the prototype/app-shell branch, so there is no production build to hide it from.
  // Alt+← / Alt+→ switch, so plain arrows stay free for navigating the variants themselves.
  let { variants, current, onPick }: { variants: { key: string; name: string }[]; current: string; onPick: (k: string) => void } = $props();
  const idx = $derived(Math.max(0, variants.findIndex((v) => v.key === current)));
  const go = (d: number) => onPick(variants[(idx + d + variants.length) % variants.length].key);
  function key(e: KeyboardEvent) {
    if (!e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === "ArrowLeft") { e.preventDefault(); go(-1); }
    if (e.key === "ArrowRight") { e.preventDefault(); go(1); }
  }
</script>

<svelte:window onkeydown={key} />
<div class="sw">
  <button onclick={() => go(-1)} aria-label="previous variant">←</button>
  <span><b>{variants[idx].key}</b> — {variants[idx].name}</span>
  <button onclick={() => go(1)} aria-label="next variant">→</button>
  <span class="hint">Alt+← →</span>
</div>

<style>
  .sw { position: fixed; bottom: 14px; left: 50%; transform: translateX(-50%); z-index: 100; display: flex; gap: 10px; align-items: center;
    background: #111; color: #fff; padding: 6px 12px; border-radius: 999px; box-shadow: 0 4px 16px rgb(0 0 0 / 0.35); font: 13px system-ui, sans-serif; opacity: 0.92; }
  button { all: unset; cursor: pointer; padding: 0 6px; font-size: 15px; }
  .hint { opacity: 0.55; font-size: 11px; }
</style>
