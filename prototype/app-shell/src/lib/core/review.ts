// PROTOTYPE — throwaway. Core: the user's Review state on one PR.
import type { RawPr, ReviewInfo, ReviewState, UserState } from "./types";

function latest(p: RawPr, me: string, user: UserState) {
  const mine = [...p.reviews.filter((r) => r.author === me && r.state !== "PENDING"), ...(user.submitted.get(p.number) ?? [])];
  return mine.sort((a, b) => a.at.localeCompare(b.at)).at(-1) ?? null;
}

export function reviewInfo(p: RawPr, me: string, user: UserState): ReviewInfo | null {
  const last = latest(p, me, user);
  const mark = user.localMarks.get(p.number);
  // A Local mark at the head wins; otherwise the latest submitted Review.
  const byMark = !!mark && (mark === p.headRefOid || !last);
  const commit = byMark ? mark! : last?.commit ?? null;
  if (!commit) return null;
  const commits = p.recentCommits ?? [];
  const i = commits.findIndex((c) => c.oid === commit);
  const diff = p.reviewDiff && p.reviewDiff.commit === commit ? p.reviewDiff : null;
  return {
    at: byMark ? null : last!.at, commit, byMark, atHead: commit === p.headRefOid,
    since: i < 0 ? null : commits.slice(i + 1), diff, gone: p.reviewDiff === null && !byMark,
  };
}

/**
 * Reviewed at the head, or the PR's own diff is unchanged since (patch-id check: merges from the
 * target branch and plain rebases don't count). Stale otherwise, including when the check couldn't run.
 */
export function reviewState(p: RawPr, me: string, user: UserState): ReviewState {
  const r = reviewInfo(p, me, user);
  if (!r) return "unreviewed";
  return r.atHead || r.diff?.same ? "reviewed" : "stale";
}
