// PROTOTYPE — throwaway. Core: CPython conventions, kept in one place.
import type { Kind, RawPr } from "./types";

const ISSUE_RE = /^(?:\[[\d.]+\]\s*)?gh-(\d+)/i;
const BACKPORT_RE = /\(GH-(\d+)\)\s*$/i;
export const BOT = "miss-islington";

export const issueOf = (title: string) => +(ISSUE_RE.exec(title)?.[1] ?? 0) || null;
export const shortTitle = (t: string) =>
  t.replace(/^\[[\d.]+\]\s*/, "").replace(/^gh-\d+:\s*/i, "").replace(BACKPORT_RE, "").trim();

/** Origin PR (targets main), Bot or Manual backport (title ends in "(GH-N)"), or other. */
export function kindOf(p: RawPr): { kind: Kind; origin: number | null } {
  if (p.baseRefName === "main") return { kind: "origin", origin: null };
  const o = +(BACKPORT_RE.exec(p.title)?.[1] ?? 0) || null;
  if (!o) return { kind: "other", origin: null };
  return { kind: p.author === BOT && p.commitCount <= 1 ? "bot" : "manual", origin: o };
}
