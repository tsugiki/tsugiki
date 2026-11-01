// PROTOTYPE — throwaway. App state: which Repository is open, and what the user does in gh-rev per
// Repository (Local marks, pins, Last seen, Pending review comments, submitted Reviews, Archive, Mute)
// as reactive maps, with core bound to them. Every "write" only changes memory; nothing reaches GitHub.
//
// Simplification: Last seen is seeded from the snapshot (2 days before it was taken) instead of a database.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { createWorkbench, type Workbench } from "$lib/core/workbench";
import { reviewInfo } from "$lib/core/review";
import type { Pr, UserState } from "$lib/core/types";
import { snapshots } from "$lib/data/source";

interface RepoState extends UserState {
  localMarks: SvelteMap<number, string>;
  pins: SvelteSet<number>;
  lastSeen: SvelteMap<number, string>;
  submitted: SvelteMap<number, { state: string; at: string; commit: string }[]>;
  archived: SvelteMap<string, string>;
  muted: SvelteSet<string>;
  mutedAuthors: SvelteSet<string>;
  pending: SvelteMap<number, { path: string; line: number; body: string }[]>;
  /** Comments the user wrote on Issues in this session (not sent: prototype). */
  posted: SvelteMap<number, { body: string; at: string }[]>;
}

function seed(s: (typeof snapshots)[number]): RepoState {
  const seen = new SvelteMap<number, string>();
  const at = new Date(new Date(s.takenAt).getTime() - 2 * 86400e3).toISOString();
  for (const p of s.prs) if (p.number % 3 !== 0) seen.set(p.number, at);
  for (const i of s.issues) if (i.number % 3 !== 0) seen.set(-i.number, at);
  return {
    localMarks: new SvelteMap(), pins: new SvelteSet(s.pinned), lastSeen: seen, submitted: new SvelteMap(),
    archived: new SvelteMap(), muted: new SvelteSet(), mutedAuthors: new SvelteSet(), pending: new SvelteMap(), posted: new SvelteMap(),
  };
}

const repos = new Map(snapshots.map((s) => {
  const user = seed(s);
  return [s.repo.name, { user, wb: createWorkbench(s, user) }] as const;
}));
export const REPOS = snapshots.map((s) => s.repo.name);
export const boardOf = (name: string): Workbench => repos.get(name)!.wb;
/** A Repository's user state, for Settings (its pins and Muted authors are written to config.toml). */
export const userOf = (name: string) => repos.get(name)!.user;

/** The open Repository, or null for the Home page. A normal launch reopens the last one. */
export const app = $state<{ repo: string | null; opened: Record<string, string>; select: string | null }>({ repo: REPOS[0] ?? null, opened: {}, select: null });
/** Open a Repository (null: the Home page), optionally with an item selected ("i:N" or "p:N"). */
export function openRepo(name: string | null, select: string | null = null) {
  app.repo = name;
  app.select = select;
  if (name) app.opened[name] = new Date().toISOString();
}

const cur = () => repos.get(app.repo ?? REPOS[0])!;
/** The open Repository's Workbench. Reading it tracks `app.repo`, so switching re-runs whatever used it. */
export const wb = new Proxy({} as Workbench, { get: (_, k) => cur().wb[k as keyof Workbench] });
/** The open Repository's user state. */
export const my = new Proxy({} as RepoState, { get: (_, k) => cur().user[k as keyof RepoState] });

export const toast = $state({ text: "", at: 0 });
export function say(text: string) { toast.text = text; toast.at = Date.now(); }

/** The user's Review state on a PR, with what it rests on (for hover text). */
export const reviewOf = (p: Pr) => reviewInfo(p, wb.me, cur().user);

// --- actions (memory only) -----------------------------------------------------------------------
export function openPr(n: number) { my.lastSeen.set(n, new Date().toISOString()); }
export function openIssue(n: number | null) { if (n) my.lastSeen.set(-n, new Date().toISOString()); }
export function toggleMark(n: number) {
  const p = wb.raw(n)!;
  if (my.localMarks.get(n) === p.headRefOid) { my.localMarks.delete(n); say(`Local mark removed from #${n}`); }
  else { my.localMarks.set(n, p.headRefOid); say(`Local mark on #${n} at ${p.headRefOid.slice(0, 7)}`); }
}
export function togglePin(n: number | null) {
  if (!n) return;
  if (my.pins.has(n)) { my.pins.delete(n); say(`Unpinned ${n}`); } else { my.pins.add(n); say(`Pinned ${n}`); }
}
export function addComment(n: number, path: string, line: number, body: string) {
  my.pending.set(n, [...(my.pending.get(n) ?? []), { path, line, body }]);
  say(`Added to your Pending review on #${n} (not sent: prototype)`);
}
export function submitReview(n: number, state: "APPROVE" | "REQUEST_CHANGES" | "COMMENT") {
  const p = wb.raw(n)!;
  my.submitted.set(n, [...(my.submitted.get(n) ?? []), { state, at: new Date().toISOString(), commit: p.headRefOid }]);
  const count = my.pending.get(n)?.length ?? 0;
  my.pending.delete(n);
  say(`Would submit ${state.toLowerCase().replace("_", " ")} on #${n} with ${count} comment(s). Nothing sent: prototype.`);
}
export function commentOnIssue(n: number, body: string) {
  my.posted.set(n, [...(my.posted.get(n) ?? []), { body, at: new Date().toISOString() }]);
  say(`Would post your comment on #${n}. Nothing sent: prototype.`);
}

// Hiding: kept in gh-rev only.
export function archive(key: string, label: string) { my.archived.set(key, new Date().toISOString()); my.muted.delete(key); say(`Archived ${label}: it comes back when something new needs you`); }
export function mute(key: string, label: string) { my.muted.add(key); my.archived.delete(key); say(`Muted ${label} until you unmute it`); }
export function muteAuthor(login: string) { my.mutedAuthors.add(login); say(`Muted ${login} in ${app.repo}`); }
export function unhide(key: string) { my.archived.delete(key); my.muted.delete(key); }
export function unmuteAuthor(login: string) { my.mutedAuthors.delete(login); }
