// PROTOTYPE — throwaway. Takes a read-only snapshot of real python/cpython data, so the UI is judged
// against real density. Stands in for the real app's Rust backend: fetching (fetch/github.mjs,
// fetch/git.mjs) and processing that needs git (Generated files, the patch-id check). Never writes to
// GitHub or to the CPython clone.
//
//   npm run proto:snapshot     (needs `gh auth login`; borrows objects from the clone at GHREV_PROTO_REPO)
//
// Output: src/lib/data/fixture.json, the only thing the app reads.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as gh from "./fetch/github.mjs";
import { openMirror } from "./fetch/git.mjs";

const ME = gh.whoami();
const REPO_DIR = process.env.GHREV_PROTO_REPO ?? "X:/Work/cpython/main";
const MIRROR_DIR = fileURLToPath(new URL("../.proto-mirror", import.meta.url));
const PINNED = [159739, 142349];
// A merged PR that touches Generated files (found via `git log -- Python/generated_cases.c.h`).
const EXTRA_DETAILS = [158282];
const FILTERS = [
  // The user's own todo search, verbatim. OR and parentheses need search type ISSUE_ADVANCED.
  { name: "Mine: author or assignee", q: `sort:updated-desc state:open (author:${ME} OR assignee:${ME})` },
  { name: "I'm involved", q: `is:pr is:open involves:${ME} -author:${ME}` },
  { name: "Jelle's open PRs", q: "is:pr is:open author:JelleZijlstra" },
  { name: "Recent typing issues", q: "is:issue is:open label:topic-typing sort:updated-desc" },
];
const MAX_ISSUES = 120;
// Same rule as src/lib/core/conventions.ts; the backend keeps its own copy (it is Rust in the real app).
const issueOf = (title) => +(/^(?:\[[\d.]+\]\s*)?gh-(\d+)/i.exec(title)?.[1] ?? 0) || null;

const prs = new Map();
const issues = new Map();
// First fetch wins: later passes add fields to these objects, so a repeat hit must not replace them.
const add = (hits) => { for (const h of hits) { const m = h.kind === "pr" ? prs : issues; if (!m.has(h.number)) m.set(h.number, h.value); } return hits; };

console.log(`snapshot as ${ME}`);
const filters = FILTERS.map((f) => {
  const hits = add(gh.search(f.q, 60));
  console.log(`  filter "${f.name}": ${hits.length}`);
  return { ...f, matches: hits.map(({ kind, number }) => ({ kind, number })) };
});
const reviewRequests = add(gh.search(`is:pr is:open review-requested:${ME}`, 50));
const reviewedBy = add(gh.search(`is:pr is:open reviewed-by:${ME}`, 50));
console.log(`  review requests: ${reviewRequests.length}, reviewed-by: ${reviewedBy.length}`);

// Every Issue reached by a matched PR, a matched issue, or a pin; then the full PR tree of each.
const wanted = new Set(PINNED);
for (const n of issues.keys()) wanted.add(n);
for (const p of prs.values()) { const i = issueOf(p.title); if (i) wanted.add(i); }
const issueNums = [...wanted].slice(0, MAX_ISSUES);
for (const i of gh.issues(issueNums)) issues.set(i.number, i);
for (const n of issueNums) {
  if (!issues.has(n)) continue;
  const tree = add(gh.search(`is:pr "gh-${n}" in:title`, n === 159739 ? 200 : 60));
  console.log(`  gh-${n}: ${tree.length} PRs`);
}

// Recent commits and CI finish time for open PRs: hover text on "stale", "reviewed" and "CI …".
const open = [...prs.values()].filter((p) => p.state === "OPEN");
for (const [n, a] of gh.prActivity(open.map((p) => p.number))) Object.assign(prs.get(n), a);
console.log(`  commits and CI times for ${open.length} open PRs`);

// Full conversation and diff for a handful of PRs, so the PR view has real content.
const pick = new Set();
const take = (hits, k) => hits.filter((h) => h.kind === "pr").slice(0, k).forEach((h) => pick.add(h.number));
take(add(gh.search(`is:pr is:open author:${ME}`, 3)), 3);
take(reviewRequests, 3);
take(reviewedBy, 2);
const tree159739 = [...prs.values()].filter((p) => issueOf(p.title) === 159739);
tree159739.filter((p) => p.state === "OPEN").slice(0, 3).forEach((p) => pick.add(p.number));
for (const p of [tree159739.find((p) => p.author === "miss-islington" && p.state === "MERGED"), tree159739.find((p) => p.baseRefName === "main" && p.state === "MERGED")])
  if (p) pick.add(p.number);
for (const n of EXTRA_DETAILS) { prs.set(n, gh.pr(n)); pick.add(n); }
const detailNums = [...pick].slice(0, 18);

// --- git: a mirror borrowing the clone's objects --------------------------------------------------
// Open PRs whose latest Review by the user isn't at the head get the patch-id check.
const lastReview = (p) => p.reviews.filter((r) => r.author === ME && r.state !== "PENDING" && r.commit).sort((a, b) => a.at.localeCompare(b.at)).at(-1);
const toCheck = open.filter((p) => { const r = lastReview(p); return r && r.commit !== p.headRefOid; });
const mirror = openMirror(MIRROR_DIR, REPO_DIR);
const branches = new Set([...toCheck, ...detailNums.map((n) => prs.get(n))].map((p) => p.baseRefName));
mirror.fetch([...branches], toCheck.map((p) => p.number));
console.log(`  mirror: ${branches.size} branches, ${toCheck.length} PR heads`);

// Patch-id check: is the PR's own diff the same as when the user reviewed it? Then merges from the
// target branch or a plain rebase don't make the Review stale. `reviewDiff: null` means the reviewed
// commit can't be fetched any more (force-pushed away); the app treats that as stale.
for (const p of toCheck) {
  const commit = lastReview(p).commit;
  if (!mirror.has(commit) && !mirror.fetchCommit(commit)) { p.reviewDiff = null; console.log(`  #${p.number}: reviewed commit ${commit.slice(0, 7)} is gone`); continue; }
  p.reviewDiff = { commit, ...mirror.diffChange(commit, p.headRefOid, p.baseRefName) };
  console.log(`  #${p.number}: ${p.reviewDiff.same ? "diff unchanged since review" : `${p.reviewDiff.changed.length} files changed since review`}`);
}

const details = {};
for (const n of detailNums) {
  const d = gh.prDetail(n);
  const gen = mirror.generated(d.files.map((f) => f.path), prs.get(n).baseRefName);
  d.files = d.files.map((f) => ({ ...f, generated: gen.has(f.path) }));
  details[n] = d;
  console.log(`  details #${n}: ${d.files.length} files${d.truncated ? ", diff truncated" : ""}`);
}

const out = {
  me: ME,
  takenAt: new Date().toISOString(),
  pinned: PINNED,
  filters,
  reviewRequests: reviewRequests.map((h) => h.number),
  issues: [...issues.values()],
  prs: [...prs.values()],
  details,
};
writeFileSync(new URL("../src/lib/data/fixture.json", import.meta.url), JSON.stringify(out));
console.log(`wrote fixture: ${out.issues.length} issues, ${out.prs.length} PRs, ${Object.keys(details).length} with details, ${toCheck.length} patch-id checks`);
