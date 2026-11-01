// PROTOTYPE — throwaway. Takes a read-only snapshot of real data for each Repository, so the UI is
// judged against real density. Stands in for the real app's Rust backend: fetching (fetch/github.mjs,
// fetch/git.mjs) and processing that needs git (Generated files, the patch-id check). Never writes to
// GitHub or to the CPython clone.
//
//   npm run proto:snapshot [owner/name ...]   (needs `gh auth login`; all Repositories by default)
//
// Output: src/lib/data/fixtures/<owner>__<name>.json, the only thing the app reads.
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as gh from "./fetch/github.mjs";
import { openMirror } from "./fetch/git.mjs";

const ME = gh.whoami();
const CPYTHON_CLONE = process.env.GHREV_PROTO_REPO ?? "X:/Work/cpython/main";
const REPOS = [
  {
    name: "python/cpython", conventions: "cpython", clone: CPYTHON_CLONE,
    pinned: [159739, 142349],
    // A merged PR that touches Generated files (found via `git log -- Python/generated_cases.c.h`).
    extraDetails: [158282],
    filters: [
      // The user's own todo search, verbatim. OR and parentheses need search type ISSUE_ADVANCED.
      { name: "Mine: author or assignee", q: `sort:updated-desc state:open (author:${ME} OR assignee:${ME})` },
      { name: "I'm involved", q: `is:pr is:open involves:${ME} -author:${ME}` },
      { name: "Jelle's open PRs", q: "is:pr is:open author:JelleZijlstra" },
      { name: "Recent typing issues", q: "is:issue is:open label:topic-typing sort:updated-desc" },
    ],
  },
  // The user's own projects: no Filters written yet, so they get the starter Filters for their role.
  { name: "sergey-miryanov/pep848-sim", conventions: "github" },
  { name: "sergey-miryanov/gcmon", conventions: "github" },
  { name: "sergey-miryanov/gc-scope", conventions: "github" },
];
const STARTER = {
  maintained: [{ name: "Open Issues", q: "is:issue is:open" }, { name: "Open PRs", q: "is:pr is:open" }],
  other: [{ name: "Mine", q: "is:open author:@me" }, { name: "Involved", q: "is:open involves:@me -author:@me" }],
};
// First sync: a small Repository loads everything open; a large one the latest updates only.
const SMALL = 300;
const RECENT_DAYS = 60;
const MAX_ISSUES = 220;
const MAX_PEEKS = 40;
const DEP_BOT = "dependabot";

const day = (n) => new Date(Date.now() - n * 86400e3).toISOString().slice(0, 10);
// Same rules as src/lib/core/conventions.ts; the backend keeps its own copy (it is Rust in the real app).
const titleIssue = (title) => +(/^(?:\[[\d.]+\]\s*)?gh-(\d+)/i.exec(title)?.[1] ?? 0) || null;
const CLOSES_RE = /\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+#(\d+)/gi;

function snapshotRepo(cfg) {
  const repo = gh.repo(cfg.name);
  const info = repo.info();
  const filtersDef = cfg.filters ?? (info.maintained ? STARTER.maintained : STARTER.other);
  console.log(`\n${cfg.name}: ${info.maintained ? "maintained" : "contributor"}, default branch ${info.defaultBranch}`);

  const prs = new Map();
  const issues = new Map();
  // First fetch wins: later passes add fields to these objects, so a repeat hit must not replace them.
  const add = (hits) => { for (const h of hits) { const m = h.kind === "pr" ? prs : issues; if (!m.has(h.number)) m.set(h.number, h.value); } return hits; };

  const filters = filtersDef.map((f) => {
    const hits = add(repo.search(f.q, cfg.conventions === "cpython" ? 60 : 200));
    console.log(`  filter "${f.name}": ${hits.length}`);
    return { ...f, matches: hits.map(({ kind, number }) => ({ kind, number })) };
  });
  const reviewRequests = add(repo.search(`is:pr is:open review-requested:${ME}`, 50));
  const reviewedBy = add(repo.search(`is:pr is:open reviewed-by:${ME}`, 50));
  // Conversations the user is in (opened, assigned, commented, mentioned), so a reply can reach them
  // even when no Filter matches.
  add(repo.search(`is:open involves:${ME} sort:updated-desc`, 60));

  // All issues: a small Repository loads everything open, plus recent PRs for the trees and stacks;
  // a large one only the latest page of updates, and loads older Issues on demand.
  const open = add(repo.search("is:open", SMALL + 1));
  const small = open.length <= SMALL;
  if (small) add(repo.search(`is:pr updated:>=${day(RECENT_DAYS)}`, 200));
  const allIssues = (small ? open.filter((h) => h.kind === "issue") : add(repo.search("is:issue is:open sort:updated-desc", 100))).map((h) => h.number);
  console.log(`  ${small ? "small" : "large"}: all issues ${allIssues.length}${small ? "" : " (latest page)"}`);

  // Stacked PRs: target another PR's branch. GitHub ignores their "Closes #N", so read it from the text.
  const heads = new Set([...prs.values()].map((p) => p.headRefName));
  const stacked = [...prs.values()].filter((p) => p.baseRefName !== info.defaultBranch && heads.has(p.baseRefName));
  for (const [n, text] of repo.bodies(stacked.map((p) => p.number))) prs.get(n).bodyCloses = [...text.matchAll(CLOSES_RE)].map((m) => +m[1]);
  if (stacked.length) console.log(`  stacked PRs: ${stacked.map((p) => `#${p.number}`).join(" ")}`);

  // Every Issue reached by a match, a PR, a pin or All issues; then the PRs of each.
  const linked = (p) => cfg.conventions === "cpython" ? [titleIssue(p.title)].filter(Boolean) : [...p.closes, ...(p.bodyCloses ?? [])];
  // Order matters for a large Repository's cap: pins, then Issues of open PRs, then the rest the
  // searches reached; All issues' page is fetched on top, without PR trees.
  const byPrs = [...prs.values()].sort((a, b) => +(b.state === "OPEN") - +(a.state === "OPEN")).flatMap(linked);
  const treeNums = [...new Set([...(cfg.pinned ?? []), ...byPrs, ...[...issues.keys()].filter((n) => !allIssues.includes(n)), ...allIssues])].slice(0, small ? 1000 : MAX_ISSUES);
  const issueNums = [...new Set([...treeNums, ...allIssues])];
  for (const i of repo.issues(issueNums)) issues.set(i.number, i);
  if (cfg.conventions === "cpython")
    for (const n of treeNums) {
      if (!issues.has(n)) continue;
      add(repo.search(`is:pr "gh-${n}" in:title`, n === 159739 ? 200 : 60));
    }
  else {
    const missing = [...new Set([...issues.values()].flatMap((i) => i.closedBy ?? []))].filter((n) => !prs.has(n));
    for (const p of repo.prs(missing)) prs.set(p.number, p);
  }
  console.log(`  ${issues.size} issues, ${prs.size} PRs`);

  // Recent commits, CI times, comments and participants for open PRs.
  const openPrs = [...prs.values()].filter((p) => p.state === "OPEN");
  for (const [n, a] of repo.prActivity(openPrs.map((p) => p.number))) Object.assign(prs.get(n), a);

  // Full conversation and diff for a handful of PRs, so the PR view has real content.
  const pick = new Set();
  const take = (list, k) => list.filter((p) => p.kind === "pr" || p.number).slice(0, k).forEach((p) => pick.add(p.number));
  if (cfg.conventions === "cpython") {
    take(add(repo.search(`is:pr is:open author:${ME}`, 3)), 3);
    take(reviewRequests, 3);
    take(reviewedBy, 2);
    const tree = [...prs.values()].filter((p) => titleIssue(p.title) === 159739);
    tree.filter((p) => p.state === "OPEN").slice(0, 3).forEach((p) => pick.add(p.number));
    for (const p of [tree.find((p) => p.author === "miss-islington" && p.state === "MERGED"), tree.find((p) => p.baseRefName === "main" && p.state === "MERGED")])
      if (p) pick.add(p.number);
  } else {
    take(openPrs.filter((p) => !p.author.startsWith(DEP_BOT)), 6);
    take(openPrs.filter((p) => p.author.startsWith(DEP_BOT)), 2);
    take(stacked, 6);
    take([...prs.values()].filter((p) => p.state === "MERGED").sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)), 3);
  }
  for (const p of repo.prs(cfg.extraDetails ?? [])) { prs.set(p.number, p); pick.add(p.number); }
  const detailNums = [...pick].filter((n) => prs.has(n)).slice(0, 18);

  // --- git: a mirror per Repository, borrowing a clone's objects when there is one ---------------
  const lastReview = (p) => p.reviews.filter((r) => r.author === ME && r.state !== "PENDING" && r.commit).sort((a, b) => a.at.localeCompare(b.at)).at(-1);
  const toCheck = openPrs.filter((p) => { const r = lastReview(p); return r && r.commit !== p.headRefOid; });
  const mirror = openMirror(fileURLToPath(new URL(`../.proto-mirrors/${cfg.name.replace("/", "__")}`, import.meta.url)), cfg.name, cfg.clone ?? null);
  const branches = new Set([...toCheck, ...detailNums.map((n) => prs.get(n))].map((p) => p.baseRefName));
  // PR heads too: the diff view expands context from the file at the PR's head, read from the mirror.
  mirror.fetch([...branches], [...new Set([...toCheck.map((p) => p.number), ...detailNums])]);

  // Patch-id check: is the PR's own diff the same as when the user reviewed it? `reviewDiff: null`
  // means the reviewed commit can't be fetched any more (force-pushed away); the app counts it stale.
  for (const p of toCheck) {
    const commit = lastReview(p).commit;
    if (!mirror.has(commit) && !mirror.fetchCommit(commit)) { p.reviewDiff = null; continue; }
    p.reviewDiff = { commit, ...mirror.diffChange(commit, p.headRefOid, p.baseRefName) };
  }
  console.log(`  patch-id: ${toCheck.length} checks, ${toCheck.filter((p) => p.reviewDiff?.same).length} unchanged`);

  const details = {};
  for (const n of detailNums) {
    const d = repo.prDetail(n);
    const gen = mirror.generated(d.files.map((f) => f.path), prs.get(n).baseRefName);
    d.files = d.files.map((f) => ({ ...f, generated: gen.has(f.path) }));
    details[n] = d;
  }
  console.log(`  details: ${detailNums.length} PRs`);

  // Peek cards for Issues and PRs elsewhere on GitHub that descriptions and comments link to.
  const html = [
    ...[...issues.values()].flatMap((i) => [i.bodyHTML ?? "", ...(i.thread ?? []).map((c) => c.html)]),
    ...Object.values(details).flatMap((d) => [d.bodyHTML, ...d.comments.map((c) => c.html), ...d.reviews.map((r) => r.html), ...d.threads.flatMap((t) => t.comments.map((c) => c.html))]),
  ].join("\n");
  const refs = new Map();
  for (const m of html.matchAll(/https:\/\/github\.com\/([\w.-]+\/[\w.-]+)\/(?:issues|pull)\/(\d+)/g))
    if (m[1].toLowerCase() !== cfg.name.toLowerCase()) refs.set(`${m[1]}#${m[2]}`, { repo: m[1], number: +m[2] });
  const peeks = Object.fromEntries(gh.peeks([...refs.values()].slice(0, MAX_PEEKS)));
  console.log(`  peeks: ${Object.keys(peeks).length} of ${refs.size} outside links`);

  return {
    me: ME,
    takenAt: new Date().toISOString(),
    repo: { name: cfg.name, defaultBranch: info.defaultBranch, maintained: info.maintained, conventions: cfg.conventions, small },
    pinned: cfg.pinned ?? [],
    filters,
    allIssues,
    reviewRequests: reviewRequests.map((h) => h.number),
    issues: [...issues.values()],
    prs: [...prs.values()],
    details,
    peeks,
  };
}

const only = process.argv.slice(2);
const outDir = new URL("../src/lib/data/fixtures/", import.meta.url);
mkdirSync(outDir, { recursive: true });
console.log(`snapshot as ${ME}`);
for (const cfg of REPOS.filter((r) => !only.length || only.includes(r.name))) {
  const out = snapshotRepo(cfg);
  writeFileSync(new URL(`${cfg.name.replace("/", "__")}.json`, outDir), JSON.stringify(out));
  console.log(`  wrote ${cfg.name}: ${out.issues.length} issues, ${out.prs.length} PRs, ${Object.keys(out.details).length} with details`);
}
