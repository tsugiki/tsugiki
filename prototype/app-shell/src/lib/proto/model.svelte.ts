// PROTOTYPE — throwaway. One in-memory model shared by every layout variant, so the variants differ
// only in presentation. Built from fixture.json (a read-only snapshot of python/cpython).
// Writes (Local marks, pins, Pending review comments, submitting a review) only change memory.
//
// Simplifications, deliberately: a Review is "stale" when its commit isn't the PR head (no patch-id
// check); Last seen is seeded from the snapshot (2 days before it was taken) instead of a database.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import raw from "./fixture.json";

export type PrState = "OPEN" | "MERGED" | "CLOSED";
export type Ci = "SUCCESS" | "FAILURE" | "PENDING" | "ERROR" | "EXPECTED" | null;
export type ReviewState = "unreviewed" | "reviewed" | "stale";
export type Kind = "origin" | "bot" | "manual" | "other";

export interface RawPr {
  number: number; title: string; state: PrState; isDraft: boolean; url: string;
  createdAt: string; updatedAt: string; mergedAt: string | null; author: string;
  baseRefName: string; headRefOid: string; additions: number; deletions: number; changedFiles: number;
  labels: string[]; reviewRequests: string[]; assignees: string[];
  reviews: { author: string; state: string; at: string; commit: string }[];
  commitCount: number; ci: Ci; comments: number;
}
export interface RawIssue {
  number: number; title: string; state: "OPEN" | "CLOSED"; author: string; labels: string[];
  updatedAt: string; comments: number;
}
export interface Thread {
  resolved: boolean; outdated: boolean; path: string; line: number | null; startLine: number | null; side: string;
  comments: { author: string; html: string; at: string }[];
}
export interface Detail {
  bodyHTML: string;
  comments: { author: string; html: string; at: string }[];
  reviews: { author: string; state: string; html: string; at: string }[];
  threads: Thread[];
  files: { path: string; additions: number; deletions: number; generated: boolean }[];
  commits: { oid: string; headline: string; at: string; author: string }[];
  diff: string; truncated: boolean;
}
interface Fixture {
  me: string; takenAt: string; pinned: number[];
  filters: { name: string; q: string; matches: { kind: "issue" | "pr"; number: number }[] }[];
  reviewRequests: number[]; issues: RawIssue[]; prs: RawPr[]; details: Record<string, Detail>;
}
const fx = raw as unknown as Fixture;

export const ME = fx.me;
export const TAKEN_AT = fx.takenAt;
export const FILTERS = fx.filters.map((f) => ({ name: f.name, q: f.q }));

// --- CPython conventions, kept in one place ------------------------------------------------------
const ISSUE_RE = /^(?:\[[\d.]+\]\s*)?gh-(\d+)/i;
const BACKPORT_RE = /\(GH-(\d+)\)\s*$/i;
export const BOT = "miss-islington";
export const issueOf = (title: string) => +(ISSUE_RE.exec(title)?.[1] ?? 0) || null;
export const shortTitle = (t: string) =>
  t.replace(/^\[[\d.]+\]\s*/, "").replace(/^gh-\d+:\s*/i, "").replace(BACKPORT_RE, "").trim();

// --- in-memory "writes" --------------------------------------------------------------------------
export const localMarks = new SvelteMap<number, string>(); // pr -> commit marked
export const pins = new SvelteSet<number>(fx.pinned);
export const lastSeen = new SvelteMap<number, string>(); // pr or issue number -> ISO time
export const pending = new SvelteMap<number, { path: string; line: number; body: string }[]>();
export const submitted = new SvelteMap<number, { state: string; at: string; commit: string }[]>();
export const toast = $state({ text: "", at: 0 });
export function say(text: string) { toast.text = text; toast.at = Date.now(); }

const seenSeed = new Date(new Date(TAKEN_AT).getTime() - 2 * 86400e3).toISOString();
for (const p of fx.prs) if (p.number % 3 !== 0) lastSeen.set(p.number, seenSeed);

// --- derived PR view -----------------------------------------------------------------------------
export interface Pr extends RawPr {
  issue: number | null; short: string; kind: Kind; origin: number | null;
  own: boolean; requested: boolean; review: ReviewState; reviewVia: number | null;
  newActivity: boolean; attention: boolean; matchedBy: string[]; detail: Detail | null;
}

const rawPrs = new Map(fx.prs.map((p) => [p.number, p]));
export const rawIssues = new Map(fx.issues.map((i) => [i.number, i]));
const matchedBy = new Map<number, string[]>();
const issueMatchedBy = new Map<number, string[]>();
for (const f of fx.filters)
  for (const m of f.matches) {
    const map = m.kind === "pr" ? matchedBy : issueMatchedBy;
    map.set(m.number, [...(map.get(m.number) ?? []), f.name]);
  }

function kindOf(p: RawPr): { kind: Kind; origin: number | null } {
  if (p.baseRefName === "main") return { kind: "origin", origin: null };
  const o = +(BACKPORT_RE.exec(p.title)?.[1] ?? 0) || null;
  if (!o) return { kind: "other", origin: null };
  return { kind: p.author === BOT && p.commitCount <= 1 ? "bot" : "manual", origin: o };
}

function ownReview(p: RawPr): ReviewState {
  const mine = [...p.reviews.filter((r) => r.author === ME && r.state !== "PENDING"), ...(submitted.get(p.number) ?? [])];
  const mark = localMarks.get(p.number);
  if (!mine.length && !mark) return "unreviewed";
  if (mark === p.headRefOid || mine.some((r) => r.commit === p.headRefOid)) return "reviewed";
  return "stale";
}

export function pr(n: number): Pr | null {
  const p = rawPrs.get(n);
  if (!p) return null;
  const { kind, origin } = kindOf(p);
  const requested = (p.reviewRequests.includes(ME) || p.assignees.includes(ME)) && !submitted.has(n);
  let review = ownReview(p);
  let reviewVia: number | null = null;
  if (kind === "bot" && origin && rawPrs.has(origin)) { review = ownReview(rawPrs.get(origin)!); reviewVia = origin; }
  const seen = lastSeen.get(n);
  const newActivity = !!seen && p.updatedAt > seen && p.author !== BOT;
  const open = p.state === "OPEN";
  const attention = open && (!p.isDraft || requested) && (requested || review !== "reviewed" || newActivity);
  return {
    ...p, issue: issueOf(p.title), short: shortTitle(p.title), kind, origin, own: p.author === ME,
    requested, review, reviewVia, newActivity, attention, matchedBy: matchedBy.get(n) ?? [],
    detail: fx.details[String(n)] ?? null,
  };
}
export const allPrs = () => fx.prs.map((p) => pr(p.number)!);

// --- Issue trees and the Workbench ---------------------------------------------------------------
export interface OriginNode { pr: Pr | null; number: number; backports: Pr[]; attention: boolean; allMerged: boolean }
export interface IssueNode {
  number: number | null; title: string; state: string; labels: string[];
  origins: OriginNode[]; matched: Pr[]; attention: number; open: number; total: number;
  /** Origins with anything open, matched or requested; the rest fold into one "merged" line. */
  live: OriginNode[]; merged: OriginNode[]; mergedCount: number;
  pinned: boolean; reasons: string[]; newActivity: boolean;
}

function tree(prs: Pr[]): OriginNode[] {
  const byNum = new Map(prs.map((p) => [p.number, p]));
  const nodes = new Map<number, OriginNode>();
  const node = (n: number) => {
    if (!nodes.has(n)) nodes.set(n, { pr: byNum.get(n) ?? null, number: n, backports: [], attention: false, allMerged: false });
    return nodes.get(n)!;
  };
  for (const p of prs) {
    if (p.kind === "origin" || p.kind === "other" || !p.origin) node(p.number);
    else node(p.origin).backports.push(p);
  }
  for (const o of nodes.values()) {
    o.backports.sort((a, b) => b.baseRefName.localeCompare(a.baseRefName, undefined, { numeric: true }));
    const all = [o.pr, ...o.backports].filter(Boolean) as Pr[];
    o.attention = all.some((p) => p.attention);
    o.allMerged = all.every((p) => p.state !== "OPEN");
  }
  const matched = (o: OriginNode) => (o.pr?.matchedBy.length ? 1 : 0);
  return [...nodes.values()].sort((a, b) => matched(b) - matched(a) || +b.attention - +a.attention || b.number - a.number);
}

export function issueNode(n: number | null, prsOverride?: Pr[]): IssueNode {
  const prs = prsOverride ?? allPrs().filter((p) => p.issue === n);
  const i = n ? rawIssues.get(n) : null;
  const reasons = [...(n ? issueMatchedBy.get(n) ?? [] : []), ...new Set(prs.flatMap((p) => p.matchedBy))];
  if (n && pins.has(n)) reasons.unshift("Pinned");
  if (prs.some((p) => p.state === "OPEN" && p.review !== "unreviewed" && p.kind !== "bot")) reasons.push("You reviewed");
  const origins = tree(prs);
  const folds = (o: OriginNode) => o.allMerged && ![o.pr, ...o.backports].some((p) => p?.matchedBy.length || p?.requested);
  const merged = origins.filter(folds);
  return {
    number: n, title: i?.title ?? (n ? `gh-${n}` : "No issue"), state: i?.state ?? "OPEN", labels: i?.labels ?? [],
    origins, live: origins.filter((o) => !folds(o)), merged,
    mergedCount: merged.reduce((a, o) => a + (o.pr ? 1 : 0) + o.backports.length, 0), matched: prs.filter((p) => p.matchedBy.length), pinned: !!n && pins.has(n),
    attention: prs.filter((p) => p.attention).length, open: prs.filter((p) => p.state === "OPEN").length,
    total: prs.length, reasons: [...new Set(reasons)], newActivity: prs.some((p) => p.newActivity),
  };
}

export type Scope = { kind: "all" } | { kind: "filter"; name: string } | { kind: "pinned" } | { kind: "view"; view: View };
export type View = "requests" | "stale" | "activity";
export const VIEWS: { key: View; name: string }[] = [
  { key: "requests", name: "Review requests" },
  { key: "stale", name: "Stale reviews" },
  { key: "activity", name: "New activity" },
];

/** Issues on the Workbench for a scope, split into active and quiet; No issue last. */
export function workbench(scope: Scope = { kind: "all" }): { active: IssueNode[]; quiet: IssueNode[]; noIssue: IssueNode | null } {
  const prs = allPrs();
  const want = new Set<number>();
  let noIssuePrs: Pr[] = [];
  const reach = (filterName?: string) => {
    for (const f of fx.filters) {
      if (filterName && f.name !== filterName) continue;
      for (const m of f.matches) {
        if (m.kind === "issue") want.add(m.number);
        else {
          const p = pr(m.number);
          if (p?.issue) want.add(p.issue);
          else if (p) noIssuePrs.push(p);
        }
      }
    }
  };
  if (scope.kind === "all") {
    reach();
    pins.forEach((n) => want.add(n));
    for (const p of prs) if (p.state === "OPEN" && p.review !== "unreviewed" && p.issue) want.add(p.issue);
  } else if (scope.kind === "filter") reach(scope.name);
  else if (scope.kind === "pinned") pins.forEach((n) => want.add(n));
  else {
    const hit = prs.filter((p) => viewHas(scope.view, p));
    hit.forEach((p) => (p.issue ? want.add(p.issue) : noIssuePrs.push(p)));
  }
  const nodes = [...want].map((n) => issueNode(n));
  noIssuePrs = [...new Map(noIssuePrs.map((p) => [p.number, p])).values()];
  const byRecent = (a: IssueNode, b: IssueNode) => +b.pinned - +a.pinned || b.attention - a.attention || (b.number ?? 0) - (a.number ?? 0);
  return {
    active: nodes.filter((i) => i.attention > 0 || i.pinned).sort(byRecent),
    quiet: nodes.filter((i) => i.attention === 0 && !i.pinned).sort(byRecent),
    noIssue: noIssuePrs.length ? issueNode(null, noIssuePrs) : null,
  };
}

export function viewHas(v: View, p: Pr) {
  if (v === "requests") return p.requested && p.state === "OPEN";
  if (v === "stale") return p.review === "stale" && p.state === "OPEN" && p.kind !== "bot";
  return p.newActivity;
}
export const viewCount = (v: View) => allPrs().filter((p) => viewHas(v, p)).length;
export const filterCount = (name: string) => fx.filters.find((f) => f.name === name)?.matches.length ?? 0;

// --- actions (memory only) -----------------------------------------------------------------------
export function openPr(n: number) { lastSeen.set(n, new Date().toISOString()); }
export function openIssue(n: number | null) { if (n) lastSeen.set(-n, new Date().toISOString()); }
export function toggleMark(n: number) {
  const p = rawPrs.get(n)!;
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
  const p = rawPrs.get(n)!;
  submitted.set(n, [...(submitted.get(n) ?? []), { state, at: new Date().toISOString(), commit: p.headRefOid }]);
  const count = pending.get(n)?.length ?? 0;
  pending.delete(n);
  say(`Would submit ${state.toLowerCase().replace("_", " ")} on #${n} with ${count} comment(s). Nothing sent: prototype.`);
}

export const ago = (iso: string | null | undefined) => {
  if (!iso) return "";
  const s = (new Date(TAKEN_AT).getTime() - new Date(iso).getTime()) / 1000;
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  if (s < 86400 * 60) return `${Math.round(s / 86400)}d`;
  return `${Math.round(s / 86400 / 30)}mo`;
};
