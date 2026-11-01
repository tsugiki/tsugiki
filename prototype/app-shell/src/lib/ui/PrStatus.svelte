<script lang="ts">
  // PROTOTYPE — throwaway. Compact PR status: words only for what needs saying; "unreviewed" stays silent.
  import type { Pr } from "$lib/core/types";
  import { reviewOf, wb } from "$lib/state/app.svelte";
  import { reviewTip, ciTip } from "./format";
  let { pr }: { pr: Pr } = $props();

  const words = $derived.by(() => {
    const w: { t: string; cls: string; tip: string }[] = [];
    const open = pr.state === "OPEN";
    const tip = reviewTip(pr, reviewOf(pr));
    if (pr.requested) w.push({ t: "requested", cls: "req", tip: "You are a requested reviewer or assignee" });
    if (pr.why === "maintainer") w.push({ t: "waits on you", cls: "req", tip: "Your Repository: open until you merge, close or review it" });
    if (pr.kind === "manual") w.push({ t: "manual", cls: "dim", tip: "Manual backport: reviewed on its own, not via its Origin PR" });
    if (pr.stackedOn) w.push({ t: `stacked on #${pr.stackedOn}`, cls: "dim", tip: `Targets #${pr.stackedOn}'s branch, ${pr.baseRefName}` });
    if (pr.stackIssue) w.push({ t: `not on ${wb.repo.defaultBranch}`, cls: "bad", tip: pr.stackIssue === "orphaned"
      ? `#${pr.stackedOn} has merged, but this PR still targets its branch: merging it won't reach the default branch`
      : `Merged into #${pr.stackedOn}'s branch after #${pr.stackedOn} had merged: its commits never reached the default branch` });
    if (pr.kind !== "bot") {
      if (pr.review === "stale") w.push({ t: "stale", cls: "bad", tip });
      else if (pr.review === "reviewed" && open) w.push({ t: "reviewed", cls: "good", tip });
    }
    if (pr.talk) w.push({ t: "reply", cls: "acc", tip: "Someone posted since you last looked, and you're in the conversation" });
    else if (pr.newActivity) w.push({ t: "new", cls: "acc", tip: "New activity since you last opened it" });
    const ciMatters = open && (pr.own || pr.dep || pr.review !== "unreviewed");
    if (pr.ci === "FAILURE" || pr.ci === "ERROR") { if (ciMatters) w.push({ t: "CI failed", cls: "bad", tip: ciTip(pr) }); }
    else if ((pr.own || pr.dep) && open && pr.ci === "PENDING") w.push({ t: "CI running", cls: "warn", tip: ciTip(pr) });
    else if ((pr.own || pr.dep) && open && pr.ci === "SUCCESS") w.push({ t: pr.dep ? "ready to merge" : "CI passed", cls: "good", tip: ciTip(pr) });
    return w;
  });
</script>

{#each words as w}<span class="w {w.cls}" title={w.tip}>{w.t}</span>{/each}

<style>
  .w { font-size: 11.5px; white-space: nowrap; }
  .w[title] { cursor: help; }
  .w + .w::before { content: "· "; color: var(--muted); }
  .req { color: var(--accent); font-weight: 600; }
  .bad { color: var(--bad); }
  .good { color: var(--good); }
  .warn { color: var(--warn); }
  .acc { color: var(--accent); }
  .dim { color: var(--muted); }
</style>
