// The two ends of a PR's diff, which the Files tab and the rail's Search both name, and the one way to tell
// one diff from another.

/** The ends of a diff: the merge base it starts from and the head it ends at. A PR moved to another Target
 * branch keeps its head and gets another merge base (#211). */
export type DiffEnds = { base: string; head: string };

/** What tells the diff between `ends` from any other: two diffs are the same when their keys are. */
export function diffKey(ends: DiffEnds): string {
  return `${ends.base}..${ends.head}`;
}
