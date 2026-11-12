// The Workbench's words for what the core says about an Issue or a PR.
import {
  treeRows,
  type Attention,
  type BackportKind,
  type CiState,
  type IssueItem,
  type ListedItem,
  type PrState,
  type ReviewState,
  type TreeEntry,
  type WorkbenchRow,
  type WorkReason,
  type Why,
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

/** How many comments an Issue, a PR or a review thread has. */
export const commentCount = (count: number) => (count === 1 ? "1 comment" : `${String(count)} comments`);

/** An Issue's or a PR's first line. */
export const headline = (item: { number: number; title: string }) => `#${String(item.number)} ${item.title}`;

const prCount = (count: number) => (count === 1 ? "1 PR" : `${String(count)} PRs`);

/** An item's first line: the people an Issue involves, or a PR's author. */
export const peopleOf = (item: ListedItem) =>
  item.kind === "issue" ? item.people.join(", ") : (item.author ?? "deleted user");

/** How many PRs an Issue's tree holds, Backports included. */
export const treeCount = (issue: IssueItem) => prCount(treeRows(issue.pull_requests).length);

/** An item's third line while nothing in it waits on the user: how many of an Issue's PRs are open, or a
 * PR's facts. */
export function summary(item: ListedItem): string {
  if (item.kind !== "issue") return details(item);
  return item.pull_requests.length > 0 ? openOfAll(item.pull_requests) : "No PR yet";
}

/** The count beside an Issue's Origin PRs: its open PRs of all of them, Backports included. */
export function openOfAll(entries: TreeEntry[]): string {
  const rows = treeRows(entries);
  const open = rows.filter((row) => row.state === "OPEN").length;
  return `${String(open)} open of ${prCount(rows.length)}`;
}

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
