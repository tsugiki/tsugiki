<!-- PROTOTYPE, throwaway (#367): the floating bar that switches the variant (Ctrl+Shift+← / →), picks what
     the next write pretends, and says what the last action would have sent to GitHub. Shown only in a
     development build. -->
<script lang="ts">
  import { proto, SIMULATIONS, VARIANTS, type Simulate } from "./pending-review.svelte";

  const index = $derived(VARIANTS.findIndex((one) => one.key === proto.variant));
  const current = $derived(VARIANTS[index]);
  const step = (by: number) => {
    const next = VARIANTS[(index + by + VARIANTS.length) % VARIANTS.length];
    if (next) proto.setVariant(next.key);
  };
  const onkeydown = (e: KeyboardEvent) => {
    if (e.ctrlKey && e.shiftKey && !e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      step(e.key === "ArrowLeft" ? -1 : 1);
    }
  };
</script>

<svelte:window {onkeydown} />

{#if import.meta.env.DEV}
  <div class="switcher" role="toolbar" aria-label="Prototype variants">
    <button type="button" aria-label="Previous variant" onclick={() => step(-1)}>‹</button>
    <span class="label">{current?.key} — {current?.name}</span>
    <button type="button" aria-label="Next variant" onclick={() => step(1)}>›</button>
    <label>
      Pretend
      <select value={proto.simulate} onchange={(e) => proto.setSimulate(e.currentTarget.value as Simulate)}>
        {#each SIMULATIONS as one (one.key)}
          <option value={one.key}>{one.name}</option>
        {/each}
      </select>
    </label>
    {#if proto.note}<span class="note" title={proto.note}>{proto.note}</span>{/if}
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
    min-width: 210px;
    text-align: center;
  }

  select {
    font: inherit;
  }

  .note {
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.9;
  }
</style>
