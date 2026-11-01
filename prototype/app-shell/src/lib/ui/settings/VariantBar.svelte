<script lang="ts">
  // PROTOTYPE — throwaway. The floating variant switcher: ← / → (not while typing) or the arrows.
  // Not part of any design; it goes away with the prototype.
  let { variants, current, onChange }: { variants: [string, string][]; current: string; onChange: (v: string) => void } = $props();
  const i = $derived(Math.max(0, variants.findIndex(([k]) => k === current)));
  const step = (d: number) => onChange(variants[(i + d + variants.length) % variants.length][0]);
  function key(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (e.ctrlKey || e.altKey || e.metaKey || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable) return;
    if (e.key === "ArrowLeft") step(-1); else if (e.key === "ArrowRight") step(1);
  }
</script>

<svelte:window onkeydown={key} />

<div class="bar">
  <button onclick={() => step(-1)} title="Previous variant (←)">←</button>
  <span><b>{variants[i][0]}</b> — {variants[i][1]}</span>
  <button onclick={() => step(1)} title="Next variant (→)">→</button>
</div>

<style>
  .bar { position: fixed; bottom: 16px; left: 50%; transform: translateX(-50%); z-index: 100; display: flex; gap: 10px; align-items: center;
    background: #111; color: #fff; border-radius: 999px; padding: 6px 8px; box-shadow: 0 6px 24px rgb(0 0 0 / 0.4); font: 13px var(--sans); border: 1px solid #444; }
  button { all: unset; cursor: pointer; width: 26px; height: 26px; border-radius: 50%; display: grid; place-items: center; background: #333; }
  button:hover { background: #555; }
  span { padding: 0 4px; white-space: nowrap; }
</style>
