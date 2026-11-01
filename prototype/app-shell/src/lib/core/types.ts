// PROTOTYPE — throwaway. Core: plain TypeScript, no Svelte, no DOM. The snapshot's shapes (what the
// backend hands the app) and the shapes core derives from them.

export type PrState = "OPEN" | "MERGED" | "CLOSED";
export type Ci = "SUCCESS" | "FAILURE" | "PENDING" | "ERROR" | "EXPECTED" | null;
export type ReviewState = "unreviewed" | "reviewed" | "stale";
export type Kind = "origin" | "bot" | "manual" | "other";

export interface RepoInfo {
  name: string; defaultBranch: string;
  /** The user administers it on GitHub: a Maintained repository. */
  maintained: boolean;
  /** How PRs name their Issues: CPython's title prefix, or GitHub's "Fixes #N". */
  conventions: "cpython" | "github";
  /** Small enough that the first sync loaded everything open. */
  small: boolean;
}

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
  baseRefName: string; headRefName: string; headRefOid: string; additions: number; deletions: number; changedFiles: number;
  labels: string[]; reviewRequests: string[]; assignees: string[];
  reviews: { author: string; state: string; at: string; commit: string }[];
  commitCount: number; ci: Ci; comments: number;
  /** GitHub's "Fixes #N" links (only on PRs into the default branch). */
  closes: number[];
  /** "Closes #N" read from a Stacked PR's description, which GitHub ignores. */
  bodyCloses?: number[];
  /** Open PRs only: the last 30 commits, and when CI last finished on the head. */
  recentCommits?: Commit[];
  ciAt?: string | null;
  /** Open PRs only: the last comments (plain text) and who has taken part. */
  posts?: { author: string; at: string; text: string }[];
  participants?: string[];
  /** Open PRs whose latest Review by the user isn't at the head; null if that commit is gone. */
  reviewDiff?: ReviewDiff | null;
}
export interface Post { author: string; html: string; at: string }
export interface RawIssue {
  number: number; title: string; state: "OPEN" | "CLOSED"; url: string; author: string; labels: string[];
  assignees: string[]; createdAt: string; updatedAt: string; comments: number;
  bodyHTML?: string; thread?: Post[]; participants?: string[];
  /** PRs GitHub links as closing this Issue. */
  closedBy?: number[];
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
/** An Issue or PR elsewhere on GitHub, as much as a Peek shows. */
export interface PeekCard {
  repo: string; number: number; kind: "issue" | "pr"; title: string; state: string; url: string;
  createdAt: string; author: string; bodyHTML: string; comments: number; last: Post[];
}
export interface FilterDef { name: string; q: string }
export interface Snapshot {
  me: string; takenAt: string; repo: RepoInfo; pinned: number[];
  filters: (FilterDef & { matches: { kind: "issue" | "pr"; number: number }[] })[];
  /** What All issues has loaded: everything open in a small Repository, the latest page in a large one. */
  allIssues: number[];
  reviewRequests: number[]; issues: RawIssue[]; prs: RawPr[]; details: Record<string, Detail>;
  peeks: Record<string, PeekCard>;
}

/** What the user did in gh-rev, for one Repository. Core only reads it; the state layer owns it. */
export interface UserState {
  localMarks: ReadonlyMap<number, string>; // PR -> commit marked
  pins: ReadonlySet<number>;
  lastSeen: ReadonlyMap<number, string>; // PR number, or -Issue number -> ISO time
  submitted: ReadonlyMap<number, { state: string; at: string; commit: string }[]>;
  /** Item key ("i:N", "p:N", "deps") -> when it was Archived. */
  archived: ReadonlyMap<string, string>;
  muted: ReadonlySet<string>;
  mutedAuthors: ReadonlySet<string>;
}

export interface Pr extends RawPr {
  /** The Issues it fixes; `issue` is the first, or null for a Standalone PR. */
  issues: number[]; issue: number | null;
  short: string; kind: Kind; origin: number | null;
  own: boolean; requested: boolean; review: ReviewState; reviewVia: number | null;
  newActivity: boolean;
  /** Someone else posted since Last seen, and the user is in the conversation. */
  talk: boolean;
  /** Opened by a dependency bot. */
  dep: boolean;
  /** The PR it is stacked on, and what is wrong with the stack. */
  stackedOn: number | null; stackIssue: "orphaned" | "merged-orphaned" | null;
  attention: boolean; why: Waiting["reason"] | null; matchedBy: string[]; detail: Detail | null;
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

export type Waiting = { pr: number | null; reason: "requested" | "stale" | "ci" | "activity" | "talk" | "maintainer" | "deps-ready" | "deps-failed" };
export interface OriginNode { pr: Pr | null; number: number; backports: Pr[]; attention: boolean; allMerged: boolean }
export interface IssueNode {
  number: number; title: string; state: string; labels: string[]; author: string | null;
  origins: OriginNode[]; matched: Pr[]; attention: number; open: number; total: number;
  /** Origins with anything open, matched or requested; the rest fold into one "merged" line. */
  live: OriginNode[]; merged: OriginNode[]; mergedCount: number;
  /** Someone else posted on the Issue since Last seen, and the user is in the conversation. */
  talk: boolean;
  people: string[]; updatedAt: string; waiting: Waiting[];
  pinned: boolean; reasons: string[]; newActivity: boolean;
}

export type Hidden = "archived" | "muted" | "author";
/** One item in the Issue list: an Issue, a Standalone PR, or the Dependency updates. */
export interface WorkItem {
  key: string; kind: "issue" | "pr" | "deps";
  number: number | null; title: string; author: string | null;
  people: string[]; updatedAt: string; total: number; open: number; mergedCount: number;
  waiting: Waiting[]; attention: number; pinned: boolean; reasons: string[];
  /** Why it is hidden, if it is; a Review request or an assignment always shows. */
  hidden: Hidden | null;
  issue: IssueNode | null; prs: Pr[];
}

export type View = "requests" | "stale" | "activity";
export type Scope =
  | { kind: "all" } | { kind: "filter"; name: string } | { kind: "pinned" } | { kind: "view"; view: View }
  | { kind: "hidden" }
  /** All issues: the open Issues and PRs the Repository has loaded, narrowed by a typed search. */
  | { kind: "issues"; q: string }
  /** A temporary Filter, `author:<login>`, opened by clicking a person. Here it only sees the snapshot. */
  | { kind: "author"; login: string };

/** Where a link in a description or comment should go. */
export type LinkTarget =
  | { kind: "here"; type: "issue" | "pr"; number: number }
  | { kind: "peek"; repo: string; number: number; added: boolean }
  | { kind: "web" };
