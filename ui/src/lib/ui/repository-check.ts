// The add dialog's words for what the core worked out about a Repository.
import type { Conventions, RepositoryCheck, RepositorySize } from "$lib/state/add-repository.svelte";

const CONVENTIONS: Record<Conventions, string> = { cpython: "CPython", github: "GitHub" };

const counted = (count: number, one: string, many: string) => `${String(count)} ${count === 1 ? one : many}`;

const WHAT_LOADS: Record<RepositorySize, (days: string) => string> = {
  small: (days) => `Small: Tsugiki loads everything open, plus what closed in the last ${days} days.`,
  large: (days) => `Large: Tsugiki loads what changed in the last ${days} days.`,
};

function aboutHowLong(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  return minutes === 0 ? "under a minute" : `about ${counted(minutes, "minute", "minutes")}`;
}

/** Each line the dialog shows about a checked Repository. */
export function describeCheck(check: RepositoryCheck): string[] {
  const issues = counted(check.open_issues, "open Issue", "open Issues");
  const prs = counted(check.open_prs, "open PR", "open PRs");
  const items = counted(check.first_sync_items, "Issue or PR", "Issues and PRs");
  const points = counted(check.first_sync_points, "point", "points");
  return [
    `${issues}, ${prs}.`,
    WHAT_LOADS[check.size](String(check.window_days)),
    `First sync, estimated: ${items}, ${aboutHowLong(check.first_sync_seconds)}, about ${points} of your 5,000 an hour.`,
    `Conventions: ${CONVENTIONS[check.repository.conventions]}.`,
  ];
}
