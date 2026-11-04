// The Workbench's words for what the core says about a PR.
import type { CiState, WorkbenchRow, WorkReason } from "$lib/state/workbench.svelte";

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

/** The row's first line. */
export const headline = (row: WorkbenchRow) => `#${String(row.number)} ${row.title}`;

/** The row's second line, with only the facts this PR has. */
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
