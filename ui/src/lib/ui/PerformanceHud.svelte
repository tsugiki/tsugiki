<script lang="ts">
  // The performance HUD's panel (ADR 0017): each number against its budget, red while it misses, and a
  // recording that writes a summary line for the release's notes to the log, and a line per long frame.
  import { describeCause } from "$lib/state/frame-causes";
  import { nextFrameLimit } from "$lib/state/frames";
  import { BUDGETS, type HudState, type IssueOpening, type JumpTimes } from "$lib/state/hud.svelte";

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
    {
      name: "Issue from the database",
      value: databaseWords(hud.issue),
      budget: `${String(BUDGETS.issueFromDatabase)} ms`,
      missed: over(fromDatabase(hud.issue), BUDGETS.issueFromDatabase),
    },
    {
      name: "Issue from GitHub",
      value: gitHubWords(hud.issue),
      budget: "none set",
      missed: false,
    },
    {
      name: "Jump in a diff",
      value: jumpWords(hud.jump),
      budget: `${String(BUDGETS.jump)} ms`,
      missed: over(hud.jump?.rows, BUDGETS.jump),
    },
  ]);

  /** The jump's rows, and when their spans came if they showed plain. */
  function jumpWords(jump: JumpTimes | undefined): string {
    if (jump === undefined) return "—";
    if (jump.spans === jump.rows) return ms(jump.rows);
    return `${ms(jump.rows)}, plain ${jump.spans === null ? "so far" : `until ${ms(jump.spans)}`}`;
  }

  /** The database's part, once the window has shown it. */
  function fromDatabase(issue: IssueOpening | undefined): number | undefined {
    return issue === undefined || issue.stage === "database" ? undefined : issue.fromDatabase;
  }

  function databaseWords(issue: IssueOpening | undefined): string {
    if (issue === undefined) return "—";
    return issue.stage === "database" ? "waiting" : ms(issue.fromDatabase);
  }

  function gitHubWords(issue: IssueOpening | undefined): string {
    switch (issue?.stage) {
      case undefined:
        return "—";
      case "database":
      case "github":
        return "waiting";
      case "held":
        return "held";
      case "read":
        return ms(issue.fromGitHub);
    }
  }

  /** The last long frame, with how long ago it was and its cause. */
  const longFrame = $derived.by(() => {
    const frame = hud.longFrame;
    if (frame === undefined) return undefined;
    const ago = Math.max(Math.round((hud.now - frame.end) / 1000), 0);
    return `Last long frame: ${ms(frame.duration)}, ${String(ago)} s ago, ${describeCause(frame.cause)}`;
  });
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
  {#if longFrame}
    <p class="miss">{longFrame}</p>
  {/if}
  <!-- Above the button, which stays at the panel's pinned bottom edge as these lines come and go. -->
  {#if hud.written}
    <p class="written">{`Written to tsugiki.log: ${hud.written}`}</p>
  {/if}
  {#if hud.error}
    <p role="alert">{hud.error}</p>
  {/if}
  {#if hud.recording}
    <button onclick={() => void hud.stop()}>Stop recording</button>
  {:else}
    <button
      onclick={() => {
        hud.start();
      }}>Start recording</button
    >
  {/if}
</aside>

<style>
  /* A fixed width: a panel pinned to the right that grew with its lines would move its button out from under
     the pointer (#232). The button comes last, at the pinned bottom edge, for the same reason. */
  aside {
    position: fixed;
    right: 8px;
    bottom: 32px;
    width: 360px;
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
