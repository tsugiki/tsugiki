// PROTOTYPE — throwaway. Core: plain TypeScript, no Svelte, no DOM. The snapshot's shapes (what the
// backend hands the app) and the shapes core derives from them.

export type PrState = "OPEN" | "MERGED" | "CLOSED";
export type Ci = "SUCCESS" | "FAILURE" | "PENDING" | "ERROR" | "EXPECTED" | null;
export type ReviewState = "unreviewed" | "reviewed" | "stale";
export type Kind = "origin" | "bot" | "manual" | "other";

export interface Commit { oid: string; short: string; at: string; headline: string; merge: boolean }
/**
 * Patch-id check, done by the backend: the PR's own diff at the reviewed commit vs. at the head, each
 * against its merge-base with the target branch, per file, Generated files left out, whitespace kept.
 */
export interface ReviewDiff {
  commit: string; same: boolean; changed: string[];
  /** Of `changed`, the files whose patch differs only in whitespace. They still count: in Python it can matter. */
  whitespaceOnly: string[];
  generatedChanged: string[];
}

export interface RawPr {
  number: number; title: string; state: PrState; isDraft: boolean; url: string;
  createdAt: string; updatedAt: string; mergedAt: string | null; author: string;
  baseRefName: string; headRefOid: string; additions: number; deletions: number; changedFiles: number;
  labels: string[]; reviewRequests: string[]; assignees: string[];
  reviews: { author: string; state: string; at: string; commit: string }[];
  commitCount: number; ci: Ci; comments: number;
  /** Open PRs only: the last 30 commits, and when CI last finished on the head. */
  recentCommits?: Commit[];
  ciAt?: string | null;
  /** Open PRs whose latest Review by the user isn't at the head; null if that commit is gone. */
  reviewDiff?: ReviewDiff | null;
}
export interface Post { author: string; html: string; at: string }
export interface RawIssue {
  number: number; title: string; state: "OPEN" | "CLOSED"; author: string; labels: string[];
  createdAt: string; updatedAt: string; comments: number;
  bodyHTML?: string; thread?: Post[];
}
export interface Thread {
  resolved: boolean; outdated: boolean; path: string; line: number | null; startLine: number | null; side: string;
  comments: Post[];
}
export interface Detail {
  bodyHTML: string;
  comments: Post[];
  reviews: { author: string; state: string; html: string; at: string }[];
  threads: Thread[];
  files: { path: string; additions: number; deletions: number; generated: boolean }[];
  commits: { oid: string; headline: string; at: string; author: string }[];
  diff: string; truncated: boolean;
}
export interface FilterDef { name: string; q: string }
export interface Snapshot {
  me: string; takenAt: string; pinned: number[];
  filters: (FilterDef & { matches: { kind: "issue" | "pr"; number: number }[] })[];
  reviewRequests: number[]; issues: RawIssue[]; prs: RawPr[]; details: Record<string, Detail>;
}

/** What the user did in gh-rev. Core only reads it; the state layer owns it (reactive maps there). */
export interface UserState {
  localMarks: ReadonlyMap<number, string>; // PR -> commit marked
  pins: ReadonlySet<number>;
  lastSeen: ReadonlyMap<number, string>; // PR number, or -Issue number -> ISO time
  submitted: ReadonlyMap<number, { state: string; at: string; commit: string }[]>;
}

export interface Pr extends RawPr {
  issue: number | null; short: string; kind: Kind; origin: number | null;
  own: boolean; requested: boolean; review: ReviewState; reviewVia: number | null;
  newActivity: boolean; attention: boolean; matchedBy: string[]; detail: Detail | null;
}

/** What the user's Review state rests on, for hover text. */
export interface ReviewInfo {
  /** When the user's latest Review was submitted; null when only a Local mark exists. */
  at: string | null;
  commit: string | null;
  byMark: boolean;
  atHead: boolean;
  /** Commits after the reviewed one, oldest first; null when it isn't among the recent commits. */
  since: Commit[] | null;
  /** Patch-id result, when the backend ran it for this commit. */
  diff: ReviewDiff | null;
  /** The backend found the reviewed commit gone (force-pushed away). */
  gone: boolean;
}

export type Waiting = { pr: number; reason: "requested" | "stale" | "ci" | "activity" };
export interface OriginNode { pr: Pr | null; number: number; backports: Pr[]; attention: boolean; allMerged: boolean }
export interface IssueNode {
  number: number | null; title: string; state: string; labels: string[];
  origins: OriginNode[]; matched: Pr[]; attention: number; open: number; total: number;
  /** Origins with anything open, matched or requested; the rest fold into one "merged" line. */
  live: OriginNode[]; merged: OriginNode[]; mergedCount: number;
  /** For the mail-style item: who is involved, latest update, and what is waiting on the user. */
  people: string[]; updatedAt: string; waiting: Waiting[];
  pinned: boolean; reasons: string[]; newActivity: boolean;
}

export type View = "requests" | "stale" | "activity";
export type Scope =
  | { kind: "all" } | { kind: "filter"; name: string } | { kind: "pinned" } | { kind: "view"; view: View }
  /** A temporary Filter, `author:<login>`, opened by clicking a person. Here it only sees the snapshot. */
  | { kind: "author"; login: string };
