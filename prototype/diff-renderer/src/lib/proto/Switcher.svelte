<!-- PROTOTYPE — throwaway. Floating variant switcher; ← / → also cycle. This whole app is the
     prototype and lives only on the prototype/diff-renderer branch, so it is not gated on DEV. -->
<script lang="ts">
  interface Props {
    variants: { key: string; name: string }[];
    current: string;
    onChange: (key: string) => void;
  }
  let { variants, current, onChange }: Props = $props();

  const idx = $derived(Math.max(0, variants.findIndex((v) => v.key === current)));
  const step = (d: number) => onChange(variants[(idx + d + variants.length) % variants.length].key);

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t.closest("input, textarea, select, [contenteditable]")) return;
    if (e.key === "ArrowLeft") step(-1);
    if (e.key === "ArrowRight") step(1);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="bar">
  <button onclick={() => step(-1)}>←</button>
  <span>{variants[idx].key} — {variants[idx].name}</span>
  <button onclick={() => step(1)}>→</button>
</div>

<style>
  .bar {
    position: fixed;
    bottom: 14px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 10;
    display: flex;
    gap: 12px;
    align-items: center;
    background: #111827;
    color: #fff;
    font: 600 13px system-ui, sans-serif;
    padding: 6px 10px;
    border-radius: 999px;
    box-shadow: 0 4px 16px #0006;
  }
  button {
    all: unset;
    cursor: pointer;
    padding: 2px 8px;
    border-radius: 999px;
    background: #374151;
  }
</style>
