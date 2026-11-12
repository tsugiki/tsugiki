// The Workbench's words for what the core says about an Issue or a PR.
import type {
  Attention,
  BackportKind,
  CiState,
  IssueItem,
  PrState,
  ReviewState,
  TreeEntry,
  WorkbenchRow,
  WorkReason,
  Why,
} from "$lib/state/workbench.svelte";

const REASONS: Record<WorkReason, string> = {
  review_request: "Review request",
  own_pr: "Own PR",
  reviewed: "Reviewed",
};

const STATES: Record<PrState, string> = {
  OPEN: "Open",
  CLOSED: "Closed",
  MERGED: "Merged",
};

const BACKPORTS: Record<BackportKind, string> = {
  bot: "Bot backport",
  manual: "Manual backport",
};

const CI: Record<CiState, string> = {
  EXPECTED: "CI expected",
  ERROR: "CI error",
  FAILURE: "CI failed",
  PENDING: "CI running",
  SUCCESS: "CI passed",
};

const YOUR_REVIEW: Record<ReviewState, string> = {
  PENDING: "Your Review is a draft",
  COMMENTED: "You commented",
  APPROVED: "You approved",
  CHANGES_REQUESTED: "You requested changes",
  DISMISSED: "Your Review was dismissed",
};

/** The words an Alert would use for one reason an item Needs attention. */
function saying(why: Why): string {
  switch (why.kind) {
    case "review_requested":
      return "Review requested from you";
    case "ci_failed":
      return why.checks.length > 0 ? `CI failed: ${why.checks.join(", ")}` : "CI failed";
    case "ci_passed":
      return "CI passed";
    case "changes_requested":
      return `Changes requested by ${why.by}`;
    case "approved":
      return `Approved by ${why.by}`;
    case "reviewed":
      return `Reviewed by ${why.by}`;
    case "comment":
      return `New comment from ${why.by}`;
    case "commits":
      return `New commits from ${why.by}`;
    case "ready_for_review":
      return `Ready for review, from ${why.by}`;
    case "mentioned":
      return "You were mentioned";
    case "backport_failed":
      return `Bot backport to ${why.target} failed`;
  }
}

/** The reasons an item Needs attention, the newest first. */
export const preview = (attention: Attention) => attention.why.map(saying).join(" · ");

const MINUTE = 60_000;
const MINUTES_IN_HOUR = 60;
const MINUTES_IN_DAY = 24 * MINUTES_IN_HOUR;

/** The time since `updatedAt`. A clock behind GitHub's gives a time in the future, which reads as now. */
export function ago(updatedAt: string, now: Date): string {
  const minutes = Math.floor((now.getTime() - new Date(updatedAt).getTime()) / MINUTE);
  if (minutes < 1) return "just now";
  if (minutes < MINUTES_IN_HOUR) return `${String(minutes)}m ago`;
  if (minutes < MINUTES_IN_DAY) return `${String(Math.floor(minutes / MINUTES_IN_HOUR))}h ago`;
  return `${String(Math.floor(minutes / MINUTES_IN_DAY))}d ago`;
}

/** An Issue's or a PR's first line. */
export const headline = (item: { number: number; title: string }) => `#${String(item.number)} ${item.title}`;

/** Each PR an Issue's tree holds: each entry, then the Backports beneath it. */
const rowsOf = (entries: TreeEntry[]): WorkbenchRow[] =>
  entries.flatMap((entry) => (entry.kind === "pr" ? [entry, ...entry.backports] : [entry]));

const prCount = (count: number) => (count === 1 ? "1 PR" : `${String(count)} PRs`);

/** An Issue's second line: who is involved, how many PRs its tree holds and when it last changed. */
export function issueDetails(issue: IssueItem, now: Date): string {
  return [
    ...(issue.people.length > 0 ? [issue.people.join(", ")] : []),
    prCount(rowsOf(issue.pull_requests).length),
    ago(issue.updated_at, now),
  ].join(" · ");
}

/** How many of the PRs an Issue's tree holds are open, of all of them, Backports included. */
export function openOfAll(entries: TreeEntry[]): string {
  const rows = rowsOf(entries);
  const open = rows.filter((row) => row.state === "OPEN").length;
  return `${String(open)} open of ${prCount(rows.length)}`;
}

/** A Standalone PR's second line, which has no Issue above it to say when it last changed. */
export const standaloneDetails = (row: WorkbenchRow, now: Date) =>
  `${details(row)} · ${ago(row.updated_at, now)}`;

/** A PR's second line, with only the facts this PR has. */
export function details(row: WorkbenchRow & { backport?: BackportKind }): string {
  const parts = [
    ...row.reasons.map((reason) => REASONS[reason]),
    ...(row.backport ? [BACKPORTS[row.backport]] : []),
    row.author ?? "deleted user",
    `into ${row.target_branch}`,
    STATES[row.state],
    ...(row.is_draft ? ["Draft"] : []),
    ...(row.ci ? [CI[row.ci]] : []),
  ];
  return parts.join(" · ");
}

/** A PR's line in the Issue view: its state, CI and where the user stands on it. */
export function prStateWords(row: WorkbenchRow): string {
  const parts = [
    STATES[row.state],
    ...(row.is_draft ? ["Draft"] : []),
    ...(row.ci ? [CI[row.ci]] : []),
    ...(row.reasons.includes("review_request") ? [REASONS.review_request] : []),
    ...(row.your_review ? [YOUR_REVIEW[row.your_review]] : []),
  ];
  return parts.join(" · ");
}

/** A Backport in the line its Origin PR folds them into: its branch and state. */
export const backportWords = (row: WorkbenchRow) => `${row.target_branch} ${STATES[row.state]}`;
