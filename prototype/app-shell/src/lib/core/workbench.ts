// PROTOTYPE — throwaway. Core: PRs, Issue trees, items and the Workbench of one Repository, derived
// from its Snapshot and the user's state. Pure functions of their inputs; reading `user` through
// reactive maps (state layer) makes callers re-run when the user acts.
import { BOT, isDep, issueLabel, issuesOf, kindOf, shortTitle } from "./conventions";
import { reviewState } from "./review";
import type { Hidden, IssueNode, OriginNode, Pr, RawIssue, RawPr, Scope, Snapshot, UserState, View, Waiting, WorkItem } from "./types";

export const VIEWS: { key: View; name: string }[] = [
  { key: "requests", name: "Review requests" },
  { key: "stale", name: "Stale reviews" },
  { key: "activity", name: "New activity" },
];

const failed = (p: RawPr) => p.ci === "FAILURE" || p.ci === "ERROR";

export function createWorkbench(fx: Snapshot, user: UserState) {
  const ME = fx.me;
  const repo = fx.repo;
  const rawPrs = new Map(fx.prs.map((p) => [p.number, p]));
  const rawIssues = new Map(fx.issues.map((i) => [i.number, i]));
  const matchedBy = new Map<number, string[]>();
  const issueMatchedBy = new Map<number, string[]>();
  for (const f of fx.filters)
    for (const m of f.matches) {
      const map = m.kind === "pr" ? matchedBy : issueMatchedBy;
      map.set(m.number, [...(map.get(m.number) ?? []), f.name]);
    }

  // --- stacks ----------------------------------------------------------------------------------
  const byHead = new Map<string, RawPr[]>();
  for (const p of fx.prs) byHead.set(p.headRefName, [...(byHead.get(p.headRefName) ?? []), p]);
  /** The PR whose branch this one targets: an open one if there is one, else the latest. */
  function below(p: RawPr): RawPr | null {
    if (p.baseRefName === repo.defaultBranch) return null;
    const c = (byHead.get(p.baseRefName) ?? []).filter((q) => q.number !== p.number);
    return c.find((q) => q.state === "OPEN") ?? c.sort((a, b) => b.number - a.number)[0] ?? null;
  }
  /** Open and the PR below has merged, or merged into its branch after that PR merged: not on the default branch. */
  function stackIssue(p: RawPr): Pr["stackIssue"] {
    const b = below(p);
    if (!b || b.state !== "MERGED") return null;
    if (p.state === "OPEN") return "orphaned";
    if (p.state === "MERGED" && p.mergedAt && b.mergedAt && p.mergedAt > b.mergedAt) return "merged-orphaned";
    return null;
  }

  // --- PRs -------------------------------------------------------------------------------------
  const mentions = (text: string) => text.includes(`@${ME}`);
  function pr(n: number): Pr | null {
    const p = rawPrs.get(n);
    if (!p) return null;
    const { kind, origin } = kindOf(repo, p);
    const issues = issuesOf(repo, p);
    const requested = (p.reviewRequests.includes(ME) || p.assignees.includes(ME)) && !user.submitted.has(n);
    let review = reviewState(p, ME, user);
    let reviewVia: number | null = null;
    if (kind === "bot" && origin && rawPrs.has(origin)) { review = reviewState(rawPrs.get(origin)!, ME, user); reviewVia = origin; }
    const dep = isDep(p);
    const seen = user.lastSeen.get(n);
    const newActivity = !!seen && p.updatedAt > seen && p.author !== BOT && !dep;
    const open = p.state === "OPEN";
    const own = p.author === ME;
    const inTalk = own || p.assignees.includes(ME) || (p.participants ?? []).includes(ME);
    const talk = !!seen && (p.posts ?? []).some((c) => c.author !== ME && c.at > seen && (inTalk || mentions(c.text)));
    // Needs attention = waiting on the user. A Bot backport's staleness is carried by its Origin PR; a
    // colleague's Draft PR only counts via a request; a Muted author's PR only via a request.
    let why: Waiting["reason"] | null =
      open && requested ? "requested" :
      open && review === "stale" && kind !== "bot" ? "stale" :
      open && own && failed(p) ? "ci" :
      repo.maintained && open && dep && p.ci === "SUCCESS" ? "deps-ready" :
      repo.maintained && open && dep && failed(p) ? "deps-failed" :
      repo.maintained && open && !own && !dep && review === "unreviewed" ? "maintainer" :
      talk ? "talk" :
      newActivity && (own || review !== "unreviewed") ? "activity" : null;
    if (p.isDraft && !own && !requested) why = null;
    if (user.mutedAuthors.has(p.author) && !requested) why = null;
    return {
      ...p, issues, issue: issues[0] ?? null, short: shortTitle(p.title), kind, origin, own,
      requested, review, reviewVia, newActivity, talk, dep, stackedOn: below(p)?.number ?? null, stackIssue: stackIssue(p),
      attention: why != null, why, matchedBy: matchedBy.get(n) ?? [], detail: fx.details[String(n)] ?? null,
    };
  }
  const allPrs = () => fx.prs.map((p) => pr(p.number)!);

  /** A Stacked PR's whole stack, bottom first. */
  function stack(n: number): Pr[] {
    const down: Pr[] = [];
    for (let p = pr(n); p; p = p.stackedOn ? pr(p.stackedOn) : null) down.unshift(p);
    const up: Pr[] = [];
    for (let top = down.at(-1)!; ; ) {
      const next = fx.prs.find((q) => q.baseRefName === top.headRefName && q.number !== top.number && below(q)?.number === top.number);
      if (!next) break;
      top = pr(next.number)!;
      up.push(top);
    }
    const all = [...down, ...up];
    return all.length > 1 ? all : [];
  }

  // --- Issues ----------------------------------------------------------------------------------
  /** Someone else posted on the Issue since Last seen, and the user opened it, is assigned, took part or is mentioned. */
  function issueTalk(i: RawIssue | undefined) {
    const seen = i && user.lastSeen.get(-i.number);
    if (!i || !seen) return false;
    const fresh = (i.thread ?? []).filter((c) => c.author !== ME && c.at > seen);
    if (!fresh.length) return false;
    return i.author === ME || i.assignees.includes(ME) || (i.participants ?? []).includes(ME) || fresh.some((c) => c.html.includes(`>@${ME}<`));
  }

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

  function issueNode(n: number, all: Pr[] = allPrs()): IssueNode {
    const prs = all.filter((p) => p.issues.includes(n));
    const i = rawIssues.get(n);
    const pinned = user.pins.has(n);
    const talk = issueTalk(i);
    const reasons = [...(issueMatchedBy.get(n) ?? []), ...new Set(prs.flatMap((p) => p.matchedBy))];
    if (pinned) reasons.unshift("Pinned");
    if (prs.some((p) => p.state === "OPEN" && p.review !== "unreviewed" && p.kind !== "bot")) reasons.push("You reviewed");
    const origins = tree(prs);
    const folds = (o: OriginNode) => o.allMerged && ![o.pr, ...o.backports].some((p) => p?.matchedBy.length || p?.requested);
    const merged = origins.filter(folds);
    const waiting: Waiting[] = [...(talk ? [{ pr: null, reason: "talk" as const }] : []), ...prs.filter((p) => p.attention).map((p) => ({ pr: p.number, reason: p.why! }))];
    return {
      number: n, title: i?.title ?? issueLabel(repo, n), state: i?.state ?? "OPEN", labels: i?.labels ?? [], author: i?.author ?? null,
      people: [...new Set([...(i ? [i.author] : []), ...prs.filter((p) => p.author !== BOT).map((p) => p.author)])].slice(0, 3),
      updatedAt: [i?.updatedAt ?? "", ...prs.map((p) => p.updatedAt)].sort().at(-1) ?? "",
      waiting, talk, origins, live: origins.filter((o) => !folds(o)), merged,
      mergedCount: merged.reduce((a, o) => a + (o.pr ? 1 : 0) + o.backports.length, 0), matched: prs.filter((p) => p.matchedBy.length), pinned,
      attention: waiting.length, open: prs.filter((p) => p.state === "OPEN").length,
      total: prs.length, reasons: [...new Set(reasons)], newActivity: prs.some((p) => p.newActivity),
    };
  }

  // --- items -----------------------------------------------------------------------------------
  function hiddenOf(it: Omit<WorkItem, "hidden">): Hidden | null {
    const asked = it.prs.some((p) => p.requested && p.state === "OPEN") || (it.kind === "issue" && !!rawIssues.get(it.number!)?.assignees.includes(ME));
    if (asked) return null;
    if (user.muted.has(it.key)) return "muted";
    const at = user.archived.get(it.key);
    if (at && !(it.attention && it.updatedAt > at)) return "archived";
    if (it.author && user.mutedAuthors.has(it.author)) {
      // An Issue a Muted author opened still shows when someone else's PR in it needs attention or a Filter matches it.
      const others = it.kind === "issue" && it.prs.some((p) => p.author !== it.author && (p.attention || p.matchedBy.length));
      if (!others) return "author";
    }
    return null;
  }
  const finish = (it: Omit<WorkItem, "hidden">): WorkItem => ({ ...it, hidden: hiddenOf(it) });

  function issueItem(n: number, all: Pr[]): WorkItem {
    const node = issueNode(n, all);
    return finish({
      key: `i:${n}`, kind: "issue", number: n, title: node.title, author: node.author, people: node.people, updatedAt: node.updatedAt,
      total: node.total, open: node.open, mergedCount: node.mergedCount, waiting: node.waiting, attention: node.attention,
      pinned: node.pinned, reasons: node.reasons, issue: node, prs: all.filter((p) => p.issues.includes(n)),
    });
  }
  function prItem(p: Pr): WorkItem {
    return finish({
      key: `p:${p.number}`, kind: "pr", number: p.number, title: p.short, author: p.author, people: [p.author], updatedAt: p.updatedAt,
      total: 1, open: +(p.state === "OPEN"), mergedCount: +(p.state === "MERGED"), waiting: p.attention ? [{ pr: p.number, reason: p.why! }] : [],
      attention: +p.attention, pinned: false, reasons: p.matchedBy, issue: null, prs: [p],
    });
  }
  function depsItem(prs: Pr[]): WorkItem {
    const waiting = prs.filter((p) => p.attention).map((p) => ({ pr: p.number, reason: p.why! }));
    return finish({
      key: "deps", kind: "deps", number: null, title: "Dependency updates", author: prs[0].author, people: [...new Set(prs.map((p) => p.author))],
      updatedAt: prs.map((p) => p.updatedAt).sort().at(-1)!, total: prs.length, open: prs.length, mergedCount: 0, waiting,
      attention: waiting.length, pinned: false, reasons: [], issue: null, prs,
    });
  }

  function viewHas(v: View, p: Pr) {
    if (v === "requests") return p.requested && p.state === "OPEN";
    if (v === "stale") return p.review === "stale" && p.state === "OPEN" && p.kind !== "bot";
    return p.newActivity;
  }

  /** Free-text search over All issues: `label:x`, `author:x`, a number, or words in the title. */
  function matches(q: string, it: { number: number; title: string; labels: string[]; author: string }) {
    const words = q.toLowerCase().split(/\s+/).filter((w) => w && !w.startsWith("is:") && !w.startsWith("sort:"));
    return words.every((w) =>
      w.startsWith("label:") ? it.labels.some((l) => l.toLowerCase() === w.slice(6)) :
      w.startsWith("author:") ? it.author.toLowerCase() === w.slice(7) :
      /^#?\d+$/.test(w) ? String(it.number).startsWith(w.replace("#", "")) : it.title.toLowerCase().includes(w));
  }
  const searchIssues = (q: string) => fx.allIssues.filter((n) => rawIssues.has(n) && matches(q, rawIssues.get(n)!));
  /** All issues lists open PRs too, each as its own row, whether or not it fixes an Issue. */
  const searchPrs = (q: string, prs: Pr[]) => prs.filter((p) => p.state === "OPEN" && matches(q, p));

  /** Items in the Issue list for a scope: active (needs attention or pinned), quiet, and hidden. */
  function workbench(scope: Scope = { kind: "all" }): { active: WorkItem[]; quiet: WorkItem[]; hidden: WorkItem[] } {
    const prs = allPrs();
    const byNum = new Map(prs.map((p) => [p.number, p]));
    const issues = new Set<number>();
    const loose = new Map<number, Pr>();
    const place = (p: Pr) => (p.issues.length ? p.issues.forEach((n) => issues.add(n)) : loose.set(p.number, p));
    const reach = (filterName?: string) => {
      for (const f of fx.filters) {
        if (filterName && f.name !== filterName) continue;
        for (const m of f.matches) {
          if (m.kind === "issue") issues.add(m.number);
          else if (byNum.has(m.number)) place(byNum.get(m.number)!);
        }
      }
    };
    if (scope.kind === "all" || scope.kind === "archived" || scope.kind === "muted") {
      reach();
      user.pins.forEach((n) => issues.add(n));
      for (const p of prs) if (p.state === "OPEN" && (p.attention || p.review !== "unreviewed")) place(p);
      for (const i of rawIssues.values()) if (issueTalk(i)) issues.add(i.number);
    } else if (scope.kind === "filter") reach(scope.name);
    else if (scope.kind === "pinned") user.pins.forEach((n) => issues.add(n));
    else if (scope.kind === "issues") {
      searchIssues(scope.q).forEach((n) => issues.add(n));
      const rows = [...issues].map((n) => issueItem(n, prs));
      rows.push(...searchPrs(scope.q, prs).map(prItem));
      return { active: rows.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)), quiet: [], hidden: [] };
    }
    else if (scope.kind === "author") {
      for (const i of rawIssues.values()) if (i.author === scope.login) issues.add(i.number);
      for (const p of prs) if (p.author === scope.login) place(p);
    } else prs.filter((p) => viewHas(scope.view, p)).forEach(place);

    // In a Maintained repository, dependency bots' open PRs fold into one Dependency updates item.
    const deps = repo.maintained ? [...loose.values()].filter((p) => p.dep && p.state === "OPEN") : [];
    deps.forEach((p) => loose.delete(p.number));
    const items = [...[...issues].map((n) => issueItem(n, prs)), ...[...loose.values()].map(prItem), ...(deps.length ? [depsItem(deps)] : [])];

    const byRecent = (a: WorkItem, b: WorkItem) => +b.pinned - +a.pinned || b.attention - a.attention || b.updatedAt.localeCompare(a.updatedAt);
    const shown = items.filter((i) => !i.hidden);
    const hidden = items.filter((i) => i.hidden);
    if (scope.kind === "archived") return { active: hidden.filter((i) => i.hidden === "archived").sort(byRecent), quiet: [], hidden: [] };
    // Muted items first, then what a Muted author hides.
    if (scope.kind === "muted")
      return { active: hidden.filter((i) => i.hidden !== "archived").sort((a, b) => +(a.hidden === "author") - +(b.hidden === "author") || byRecent(a, b)), quiet: [], hidden: [] };
    return {
      active: shown.filter((i) => i.attention > 0 || i.pinned).sort(byRecent),
      quiet: shown.filter((i) => i.attention === 0 && !i.pinned).sort(byRecent),
      hidden,
    };
  }

  const viewCount = (v: View) => allPrs().filter((p) => viewHas(v, p)).length;
  /** Like an unread count: how many items in a scope need attention. */
  const attentionCount = (s: Scope) => workbench(s).active.filter((i) => i.attention > 0).length;

  return {
    me: ME,
    takenAt: fx.takenAt,
    repo,
    filters: fx.filters.map((f) => ({ name: f.name, q: f.q })),
    allIssueCount: fx.allIssues.length,
    issue: (n: number) => rawIssues.get(n),
    issues: () => rawIssues.values(),
    raw: (n: number) => rawPrs.get(n),
    peek: (repoName: string, n: number) => fx.peeks[`${repoName}#${n}`] ?? null,
    pr, allPrs, stack, issueNode, workbench, viewHas, viewCount, attentionCount,
    /** What waits on the user here, for the Home page. */
    summary() {
      const prs = allPrs();
      const open = prs.filter((p) => p.state === "OPEN" && p.attention);
      return {
        attention: attentionCount({ kind: "all" }),
        requests: open.filter((p) => p.why === "requested").length,
        stale: open.filter((p) => p.why === "stale").length,
        ci: open.filter((p) => p.why === "ci").length,
        waiting: open.filter((p) => p.why === "maintainer").length,
        deps: open.filter((p) => p.why === "deps-ready" || p.why === "deps-failed").length,
        talk: [...rawIssues.values()].filter(issueTalk).length + prs.filter((p) => p.why === "talk").length,
      };
    },
  };
}
export type Workbench = ReturnType<typeof createWorkbench>;
