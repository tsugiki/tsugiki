// PROTOTYPE — throwaway. UI: words and hover text built from core's structured results.
import type { Pr, ReviewInfo, Waiting } from "$lib/core/types";
import { wb } from "$lib/state/app.svelte";

/** Relative time, measured from when the snapshot was taken. */
export const ago = (iso: string | null | undefined) => {
  if (!iso) return "";
  const s = (new Date(wb.takenAt).getTime() - new Date(iso).getTime()) / 1000;
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  if (s < 86400 * 60) return `${Math.round(s / 86400)}d`;
  return `${Math.round(s / 86400 / 30)}mo`;
};

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

export const waitingText = (w: Waiting) =>
  ({
    requested: `review requested on #${w.pr}`, stale: `#${w.pr} changed since your review`, ci: `CI failed on your #${w.pr}`,
    activity: `new activity on #${w.pr}`, talk: w.pr ? `new reply on #${w.pr}` : "new reply on the Issue", maintainer: `#${w.pr} waits on you`,
    "deps-ready": `#${w.pr} ready to merge`, "deps-failed": `CI failed on #${w.pr}`,
  })[w.reason];

/** Hover text on "stale" and "reviewed": the user's Review, what came after it, and the patch-id verdict. */
export function reviewTip(p: Pr, r: ReviewInfo | null): string {
  if (!r) return "";
  const lines = [r.byMark ? `Local mark at ${r.commit!.slice(0, 7)}.` : `You reviewed ${ago(r.at)} ago at ${r.commit!.slice(0, 7)}.`];
  if (r.atHead) return [...lines, "That is the current head."].join("\n");
  if (r.since) {
    const merges = r.since.filter((c) => c.merge).length;
    lines.push(`Since then: ${plural(r.since.length - merges, "new commit")}, ${plural(merges, "merge")} from main; last ${ago(r.since.at(-1)?.at)} ago.`);
    for (const c of r.since.slice(-5).reverse()) lines.push(`  ${c.short} ${c.merge ? "[merge] " : ""}${c.headline} (${ago(c.at)} ago)`);
  } else if (r.gone) lines.push(`That commit is gone (force-pushed away); head is ${p.headRefOid.slice(0, 7)}.`);
  else lines.push(`That commit is no longer among the recent commits (rebase or force-push); head is ${p.headRefOid.slice(0, 7)}.`);
  if (r.diff?.same) lines.push("The PR's diff is the same as when you reviewed it (patch-id), so this isn't stale.");
  else if (r.diff) {
    const ws = new Set(r.diff.whitespaceOnly);
    lines.push(`Changed since your review: ${plural(r.diff.changed.length, "file")}${ws.size === r.diff.changed.length ? ", whitespace only" : ""}.`);
    for (const f of r.diff.changed.slice(0, 8)) lines.push(`  ${f}${ws.has(f) ? " (whitespace only)" : ""}`);
    if (r.diff.changed.length > 8) lines.push(`  … ${r.diff.changed.length - 8} more`);
  } else if (r.gone) lines.push("The diffs can't be compared, so it counts as stale.");
  if (r.diff?.generatedChanged.length) lines.push(`Generated files changed too (not counted): ${r.diff.generatedChanged.length}.`);
  return lines.join("\n");
}

/** Hover text on the CI word: when it finished, on which head commit. */
export function ciTip(p: Pr): string {
  const word: Record<string, string> = { SUCCESS: "passed", FAILURE: "failed", ERROR: "errored", PENDING: "running", EXPECTED: "waiting" };
  const head = p.recentCommits?.at(-1);
  const when = p.ciAt ? (p.ci === "PENDING" ? `, last check started ${ago(p.ciAt)} ago` : ` ${ago(p.ciAt)} ago`) : "";
  return `CI ${word[p.ci ?? ""] ?? "unknown"}${when}` + (head ? `\non head ${head.short}, committed ${ago(head.at)} ago${head.merge ? " (merge from main)" : ""}` : "");
}
