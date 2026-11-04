// The Workbench's words for what the core says about an Issue or a PR.
import type { CiState, IssueItem, WorkbenchRow, WorkReason } from "$lib/state/workbench.svelte";

const REASONS: Record<WorkReason, string> = {
  review_request: "Review request",
  own_pr: "Own PR",
  reviewed: "Reviewed",
};

const CI: Record<CiState, string> = {
  EXPECTED: "CI expected",
  ERROR: "CI error",
  FAILURE: "CI failed",
  PENDING: "CI running",
  SUCCESS: "CI passed",
};

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

/** An Issue's second line: who is involved, how many PRs fix it and when it last changed. */
export function issueDetails(issue: IssueItem, now: Date): string {
  const count = issue.pull_requests.length;
  return [
    ...(issue.people.length > 0 ? [issue.people.join(", ")] : []),
    count === 1 ? "1 PR" : `${String(count)} PRs`,
    ago(issue.updated_at, now),
  ].join(" · ");
}

/** A Standalone PR's second line, which has no Issue above it to say when it last changed. */
export const standaloneDetails = (row: WorkbenchRow, now: Date) =>
  `${details(row)} · ${ago(row.updated_at, now)}`;

/** A PR's second line, with only the facts this PR has. */
export function details(row: WorkbenchRow): string {
  const parts = [
    ...row.reasons.map((reason) => REASONS[reason]),
    row.author ?? "deleted user",
    `into ${row.target_branch}`,
    ...(row.is_draft ? ["Draft"] : []),
    ...(row.ci ? [CI[row.ci]] : []),
  ];
  return parts.join(" · ");
}
