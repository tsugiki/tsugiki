// PROTOTYPE — throwaway. App state: owns what the user does in gh-rev (Local marks, pins, Last seen,
// Pending review comments, submitted Reviews) as reactive maps, and binds core to them. Every "write"
// only changes memory; nothing reaches GitHub.
//
// Simplification: Last seen is seeded from the snapshot (2 days before it was taken) instead of a database.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { createWorkbench } from "$lib/core/workbench";
import { reviewInfo } from "$lib/core/review";
import type { Pr } from "$lib/core/types";
import { snapshot } from "$lib/data/source";

export const localMarks = new SvelteMap<number, string>();
export const pins = new SvelteSet<number>(snapshot.pinned);
export const lastSeen = new SvelteMap<number, string>();
export const submitted = new SvelteMap<number, { state: string; at: string; commit: string }[]>();
export const pending = new SvelteMap<number, { path: string; line: number; body: string }[]>();
export const toast = $state({ text: "", at: 0 });
export function say(text: string) { toast.text = text; toast.at = Date.now(); }

const seenSeed = new Date(new Date(snapshot.takenAt).getTime() - 2 * 86400e3).toISOString();
for (const p of snapshot.prs) if (p.number % 3 !== 0) lastSeen.set(p.number, seenSeed);

const user = { localMarks, pins, lastSeen, submitted };
export const wb = createWorkbench(snapshot, user);
/** The user's Review state on a PR, with what it rests on (for hover text). */
export const reviewOf = (p: Pr) => reviewInfo(p, wb.me, user);

// --- actions (memory only) -----------------------------------------------------------------------
export function openPr(n: number) { lastSeen.set(n, new Date().toISOString()); }
export function openIssue(n: number | null) { if (n) lastSeen.set(-n, new Date().toISOString()); }
export function toggleMark(n: number) {
  const p = wb.raw(n)!;
  if (localMarks.get(n) === p.headRefOid) { localMarks.delete(n); say(`Local mark removed from #${n}`); }
  else { localMarks.set(n, p.headRefOid); say(`Local mark on #${n} at ${p.headRefOid.slice(0, 7)}`); }
}
export function togglePin(n: number | null) {
  if (!n) return;
  if (pins.has(n)) { pins.delete(n); say(`Unpinned gh-${n}`); } else { pins.add(n); say(`Pinned gh-${n}`); }
}
export function addComment(n: number, path: string, line: number, body: string) {
  pending.set(n, [...(pending.get(n) ?? []), { path, line, body }]);
  say(`Added to your Pending review on #${n} (not sent: prototype)`);
}
export function submitReview(n: number, state: "APPROVE" | "REQUEST_CHANGES" | "COMMENT") {
  const p = wb.raw(n)!;
  submitted.set(n, [...(submitted.get(n) ?? []), { state, at: new Date().toISOString(), commit: p.headRefOid }]);
  const count = pending.get(n)?.length ?? 0;
  pending.delete(n);
  say(`Would submit ${state.toLowerCase().replace("_", " ")} on #${n} with ${count} comment(s). Nothing sent: prototype.`);
}
