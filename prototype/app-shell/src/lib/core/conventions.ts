// PROTOTYPE — throwaway. Core: per-Repository conventions, kept in one place. CPython names Issues in
// PR titles and has Backports; everything else uses GitHub's "Fixes #N" links.
import type { Kind, RawPr, RepoInfo } from "./types";

const ISSUE_RE = /^(?:\[[\d.]+\]\s*)?gh-(\d+)/i;
const BACKPORT_RE = /\(GH-(\d+)\)\s*$/i;
export const BOT = "miss-islington";
export const DEP_BOTS = ["dependabot", "renovate"];

export const titleIssue = (title: string) => +(ISSUE_RE.exec(title)?.[1] ?? 0) || null;
export const shortTitle = (t: string) =>
  t.replace(/^\[[\d.]+\]\s*/, "").replace(/^gh-\d+:\s*/i, "").replace(BACKPORT_RE, "").trim();
export const isDep = (p: RawPr) => DEP_BOTS.some((b) => p.author === b || p.author === `${b}[bot]` || p.author === `app/${b}`);

/** The Issues a PR fixes, in this Repository's convention. A Stacked PR's own "Closes #N" counts too. */
export function issuesOf(repo: RepoInfo, p: RawPr): number[] {
  if (repo.conventions === "cpython") return [titleIssue(p.title)].filter((n): n is number => !!n);
  return [...new Set([...p.closes, ...(p.bodyCloses ?? [])])];
}

/** Origin PR (targets the default branch), Bot or Manual backport (CPython only), or other. */
export function kindOf(repo: RepoInfo, p: RawPr): { kind: Kind; origin: number | null } {
  if (p.baseRefName === repo.defaultBranch) return { kind: "origin", origin: null };
  if (repo.conventions !== "cpython") return { kind: "other", origin: null };
  const o = +(BACKPORT_RE.exec(p.title)?.[1] ?? 0) || null;
  if (!o) return { kind: "other", origin: null };
  return { kind: p.author === BOT && p.commitCount <= 1 ? "bot" : "manual", origin: o };
}

/** How the Issue view names its PR section: CPython's Origin PRs have Backports under them. */
export const prSectionName = (repo: RepoInfo) => (repo.conventions === "cpython" ? "Origin PRs" : "PRs");
export const issueLabel = (repo: RepoInfo, n: number) => (repo.conventions === "cpython" ? `gh-${n}` : `#${n}`);
