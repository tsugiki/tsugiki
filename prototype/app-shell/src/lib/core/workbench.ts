// PROTOTYPE — throwaway. Core: PRs, Issue trees and the Workbench, derived from a Snapshot and the
// user's state. Pure functions of their inputs; reading `user` through reactive maps (state layer)
// makes callers re-run when the user acts.
import { BOT, issueOf, kindOf, shortTitle } from "./conventions";
import { reviewState } from "./review";
import type { IssueNode, OriginNode, Pr, RawPr, Scope, Snapshot, UserState, View, Waiting } from "./types";

export const VIEWS: { key: View; name: string }[] = [
  { key: "requests", name: "Review requests" },
  { key: "stale", name: "Stale reviews" },
  { key: "activity", name: "New activity" },
];

const failed = (p: RawPr) => p.ci === "FAILURE" || p.ci === "ERROR";

export function createWorkbench(fx: Snapshot, user: UserState) {
  const ME = fx.me;
  const rawPrs = new Map(fx.prs.map((p) => [p.number, p]));
  const rawIssues = new Map(fx.issues.map((i) => [i.number, i]));
  const matchedBy = new Map<number, string[]>();
  const issueMatchedBy = new Map<number, string[]>();
  for (const f of fx.filters)
    for (const m of f.matches) {
      const map = m.kind === "pr" ? matchedBy : issueMatchedBy;
      map.set(m.number, [...(map.get(m.number) ?? []), f.name]);
    }

  function pr(n: number): Pr | null {
    const p = rawPrs.get(n);
    if (!p) return null;
    const { kind, origin } = kindOf(p);
    const requested = (p.reviewRequests.includes(ME) || p.assignees.includes(ME)) && !user.submitted.has(n);
    let review = reviewState(p, ME, user);
    let reviewVia: number | null = null;
    if (kind === "bot" && origin && rawPrs.has(origin)) { review = reviewState(rawPrs.get(origin)!, ME, user); reviewVia = origin; }
    const seen = user.lastSeen.get(n);
    const newActivity = !!seen && p.updatedAt > seen && p.author !== BOT;
    const open = p.state === "OPEN";
    const own = p.author === ME;
    // Needs attention = waiting on the user: a Review request, a Stale review, failed CI on an Own PR,
    // or New activity on a PR the user reviewed or authored. A Bot backport's staleness is carried by
    // its Origin PR, so it isn't counted twice. A colleague's Draft PR only counts via a request.
    const attention =
      !(p.isDraft && !own && !requested) &&
      ((open && requested) ||
        (open && review === "stale" && kind !== "bot") ||
        (open && own && failed(p)) ||
        (newActivity && (own || review !== "unreviewed")));
    return {
      ...p, issue: issueOf(p.title), short: shortTitle(p.title), kind, origin, own,
      requested, review, reviewVia, newActivity, attention, matchedBy: matchedBy.get(n) ?? [],
      detail: fx.details[String(n)] ?? null,
    };
  }
  const allPrs = () => fx.prs.map((p) => pr(p.number)!);

  function tree(prs: Pr[]): OriginNode[] {
    const byNum = new Map(prs.map((p) => [p.number, p]));
    const nodes = new Map<number, OriginNode>();
    const node = (n: number) => {
      if (!nodes.has(n)) nodes.set(n, { pr: byNum.get(n) ?? null, number: n, backports: [], attention: false, allMerged: false });
      return nodes.get(n)!;
    };
    for (const p of prs) {
      if (p.kind === "origin" || p.kind === "other" || !p.origin) node(p.number);
      else node(p.origin).backports.push(p);
    }
    for (const o of nodes.values()) {
      o.backports.sort((a, b) => b.baseRefName.localeCompare(a.baseRefName, undefined, { numeric: true }));
      const all = [o.pr, ...o.backports].filter(Boolean) as Pr[];
      o.attention = all.some((p) => p.attention);
      o.allMerged = all.every((p) => p.state !== "OPEN");
    }
    const matched = (o: OriginNode) => (o.pr?.matchedBy.length ? 1 : 0);
    return [...nodes.values()].sort((a, b) => matched(b) - matched(a) || +b.attention - +a.attention || b.number - a.number);
  }

  const waitingOn = (p: Pr): Waiting["reason"] =>
    p.requested ? "requested" : p.review === "stale" && p.kind !== "bot" ? "stale" : p.own && failed(p) ? "ci" : "activity";

  function issueNode(n: number | null, prsOverride?: Pr[]): IssueNode {
    const prs = prsOverride ?? allPrs().filter((p) => p.issue === n);
    const i = n ? rawIssues.get(n) : null;
    const pinned = !!n && user.pins.has(n);
    const reasons = [...(n ? issueMatchedBy.get(n) ?? [] : []), ...new Set(prs.flatMap((p) => p.matchedBy))];
    if (pinned) reasons.unshift("Pinned");
    if (prs.some((p) => p.state === "OPEN" && p.review !== "unreviewed" && p.kind !== "bot")) reasons.push("You reviewed");
    const origins = tree(prs);
    const folds = (o: OriginNode) => o.allMerged && ![o.pr, ...o.backports].some((p) => p?.matchedBy.length || p?.requested);
    const merged = origins.filter(folds);
    return {
      number: n, title: i?.title ?? (n ? `gh-${n}` : "No issue"), state: i?.state ?? "OPEN", labels: i?.labels ?? [],
      people: [...new Set([...(i ? [i.author] : []), ...prs.filter((p) => p.author !== BOT).map((p) => p.author)])].slice(0, 3),
      updatedAt: [i?.updatedAt ?? "", ...prs.map((p) => p.updatedAt)].sort().at(-1) ?? "",
      waiting: prs.filter((p) => p.attention).map((p) => ({ pr: p.number, reason: waitingOn(p) })),
      origins, live: origins.filter((o) => !folds(o)), merged,
      mergedCount: merged.reduce((a, o) => a + (o.pr ? 1 : 0) + o.backports.length, 0), matched: prs.filter((p) => p.matchedBy.length), pinned,
      attention: prs.filter((p) => p.attention).length, open: prs.filter((p) => p.state === "OPEN").length,
      total: prs.length, reasons: [...new Set(reasons)], newActivity: prs.some((p) => p.newActivity),
    };
  }

  function viewHas(v: View, p: Pr) {
    if (v === "requests") return p.requested && p.state === "OPEN";
    if (v === "stale") return p.review === "stale" && p.state === "OPEN" && p.kind !== "bot";
    return p.newActivity;
  }

  /** Issues on the Workbench for a scope, split into active and quiet; No issue last. */
  function workbench(scope: Scope = { kind: "all" }): { active: IssueNode[]; quiet: IssueNode[]; noIssue: IssueNode | null } {
    const prs = allPrs();
    const want = new Set<number>();
    let noIssuePrs: Pr[] = [];
    const reach = (filterName?: string) => {
      for (const f of fx.filters) {
        if (filterName && f.name !== filterName) continue;
        for (const m of f.matches) {
          if (m.kind === "issue") want.add(m.number);
          else {
            const p = pr(m.number);
            if (p?.issue) want.add(p.issue);
            else if (p) noIssuePrs.push(p);
          }
        }
      }
    };
    if (scope.kind === "all") {
      reach();
      user.pins.forEach((n) => want.add(n));
      for (const p of prs) if (p.state === "OPEN" && p.review !== "unreviewed" && p.issue) want.add(p.issue);
    } else if (scope.kind === "filter") reach(scope.name);
    else if (scope.kind === "pinned") user.pins.forEach((n) => want.add(n));
    else if (scope.kind === "author") {
      for (const i of rawIssues.values()) if (i.author === scope.login) want.add(i.number);
      for (const p of prs) if (p.author === scope.login) (p.issue ? want.add(p.issue) : noIssuePrs.push(p));
    } else {
      const hit = prs.filter((p) => viewHas(scope.view, p));
      hit.forEach((p) => (p.issue ? want.add(p.issue) : noIssuePrs.push(p)));
    }
    const nodes = [...want].map((n) => issueNode(n));
    noIssuePrs = [...new Map(noIssuePrs.map((p) => [p.number, p])).values()];
    const byRecent = (a: IssueNode, b: IssueNode) => +b.pinned - +a.pinned || b.attention - a.attention || (b.number ?? 0) - (a.number ?? 0);
    return {
      active: nodes.filter((i) => i.attention > 0 || i.pinned).sort(byRecent),
      quiet: nodes.filter((i) => i.attention === 0 && !i.pinned).sort(byRecent),
      noIssue: noIssuePrs.length ? issueNode(null, noIssuePrs) : null,
    };
  }

  return {
    me: ME,
    takenAt: fx.takenAt,
    filters: fx.filters.map((f) => ({ name: f.name, q: f.q })),
    issue: (n: number) => rawIssues.get(n),
    issues: () => rawIssues.values(),
    raw: (n: number) => rawPrs.get(n),
    pr, allPrs, issueNode, workbench, viewHas,
    viewCount: (v: View) => allPrs().filter((p) => viewHas(v, p)).length,
    /** Like an unread count: how many Issues in a scope need attention. */
    attentionCount: (s: Scope) => workbench(s).active.filter((i) => i.attention > 0).length,
  };
}
export type Workbench = ReturnType<typeof createWorkbench>;
