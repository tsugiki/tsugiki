<script lang="ts">
  // The performance HUD's panel (ADR 0017): each number against its budget, red while it misses, and a
  // recording that writes one line to the log for the release's notes.
  import { nextFrameLimit } from "$lib/state/frames";
  import { BUDGETS, type HudState } from "$lib/state/hud.svelte";

  let { hud, refreshing }: { hud: HudState; refreshing: boolean } = $props();

  $effect(() => {
    hud.refreshing(refreshing);
  });

  const ms = (value: number | undefined, digits = 1) =>
    value === undefined ? "—" : `${value.toFixed(digits)} ms`;
  const over = (value: number | undefined, budget: number) => value !== undefined && value > budget;

  const keyLimit = $derived(nextFrameLimit(hud.interval));
  const frameBudget = `${String(BUDGETS.frame)} ms`;
  const lines = $derived([
    {
      name: "Start-up",
      value: ms(hud.startup, 0),
      budget: "1 s",
      missed: over(hud.startup, BUDGETS.startup),
    },
    { name: "Key to frame", value: ms(hud.key), budget: ms(keyLimit), missed: over(hud.key, keyLimit) },
    {
      name: "Frame, latest",
      value: ms(hud.frames?.latest),
      budget: frameBudget,
      missed: over(hud.frames?.latest, BUDGETS.frame),
    },
    {
      name: "Frame, p95",
      value: ms(hud.frames?.p95),
      budget: frameBudget,
      missed: over(hud.frames?.p95, BUDGETS.frame),
    },
    {
      name: "Frame, worst",
      value: ms(hud.frames?.worst),
      budget: frameBudget,
      missed: over(hud.frames?.worst, BUDGETS.frame),
    },
    {
      name: `Frames over ${frameBudget}`,
      value: hud.frames ? `${String(hud.frames.overLimit)} of ${String(hud.frames.count)}` : "—",
      budget: "none",
      missed: over(hud.frames?.overLimit, 0),
    },
    {
      name: "Dropped in last refresh",
      value: hud.lastRefresh === undefined ? "—" : String(hud.lastRefresh),
      budget: "none",
      missed: over(hud.lastRefresh, 0),
    },
    {
      name: "View query",
      value: ms(hud.viewQuery),
      budget: `${String(BUDGETS.viewQuery)} ms`,
      missed: over(hud.viewQuery, BUDGETS.viewQuery),
    },
  ]);
</script>

<aside aria-label="Performance HUD">
  <table>
    <thead>
      <tr><th scope="col">Measure</th><th scope="col">Now</th><th scope="col">Budget</th></tr>
    </thead>
    <tbody>
      {#each lines as line (line.name)}
        <tr class:miss={line.missed}>
          <th scope="row">{line.name}</th>
          <td>{line.value}</td>
          <td>{line.budget}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if hud.recording}
    <button onclick={() => void hud.stop()}>Stop recording</button>
  {:else}
    <button
      onclick={() => {
        hud.start();
      }}>Start recording</button
    >
  {/if}
  {#if hud.written}
    <p class="written">{`Written to tsugiki.log: ${hud.written}`}</p>
  {/if}
  {#if hud.error}
    <p role="alert">{hud.error}</p>
  {/if}
</aside>

<style>
  aside {
    position: fixed;
    right: 8px;
    bottom: 32px;
    max-width: 360px;
    padding: 6px 8px;
    font:
      12px/1.4 ui-monospace,
      monospace;
    background: rgb(255 255 255 / 95%);
    border: 1px solid #ccc;
    border-radius: 4px;
    box-shadow: 0 2px 8px rgb(0 0 0 / 15%);
  }

  table {
    border-collapse: collapse;
  }

  th,
  td {
    padding: 1px 6px 1px 0;
    font-weight: normal;
    text-align: left;
    white-space: nowrap;
  }

  thead th {
    color: #666;
  }

  td {
    text-align: right;
  }

  .miss {
    color: #c00;
  }

  button {
    margin-top: 4px;
    font: inherit;
  }

  p {
    margin: 4px 0 0;
    overflow-wrap: anywhere;
    user-select: text;
  }

  [role="alert"] {
    color: #c00;
  }
</style>
