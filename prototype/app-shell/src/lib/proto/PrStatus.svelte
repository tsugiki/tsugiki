<script lang="ts">
  // PROTOTYPE — throwaway. Compact PR status for the Issue list (replaces PrBadges in variant A):
  // one coloured state dot at the left, then words only for what needs saying.
  import type { Pr } from "./model.svelte";
  let { pr }: { pr: Pr } = $props();

  // Words appear only when they tell the user something; "unreviewed" is the default and stays silent.
  const words = $derived.by(() => {
    const w: { t: string; cls: string; tip: string }[] = [];
    const open = pr.state === "OPEN";
    if (pr.requested) w.push({ t: "requested", cls: "req", tip: "You are a requested reviewer or assignee" });
    if (pr.kind === "manual") w.push({ t: "manual", cls: "dim", tip: "Manual backport: reviewed on its own, not via its Origin PR" });
    if (pr.kind !== "bot") {
      if (pr.review === "stale") w.push({ t: "stale", cls: "bad", tip: "The PR changed since your Review" });
      else if (pr.review === "reviewed" && open) w.push({ t: "reviewed", cls: "good", tip: "You reviewed the current head" });
    }
    if (pr.newActivity) w.push({ t: "new", cls: "acc", tip: "New activity since you last opened it" });
    if (pr.ci === "FAILURE" || pr.ci === "ERROR") { if (open && (pr.own || pr.review !== "unreviewed")) w.push({ t: "CI failed", cls: "bad", tip: "CI failed at the head commit" }); }
    else if (pr.own && open && pr.ci === "PENDING") w.push({ t: "CI running", cls: "warn", tip: "CI is running" });
    else if (pr.own && open && pr.ci === "SUCCESS") w.push({ t: "CI passed", cls: "good", tip: "CI passed" });
    return w;
  });
</script>

{#each words as w}<span class="w {w.cls}" title={w.tip}>{w.t}</span>{/each}

<style>
  .w { font-size: 11.5px; white-space: nowrap; }
  .w + .w::before { content: "· "; color: var(--muted); }
  .req { color: var(--accent); font-weight: 600; }
  .bad { color: var(--bad); }
  .good { color: var(--good); }
  .warn { color: var(--warn); }
  .acc { color: var(--accent); }
  .dim { color: var(--muted); }
</style>
