// PROTOTYPE — throwaway. Takes a read-only snapshot of real python/cpython data with your gh token,
// so the UI variants are judged against real density. Never writes to GitHub.
//
//   npm run proto:snapshot            (needs `gh auth login`; reads .gitattributes from GHREV_PROTO_REPO)
//
// Output: src/lib/proto/fixture.json
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const ME = execFileSync("gh", ["api", "user", "--jq", ".login"], { encoding: "utf8" }).trim();
const REPO_DIR = process.env.GHREV_PROTO_REPO ?? "X:/Work/cpython/main";
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
const REVIEW_REQUESTS = `is:pr is:open review-requested:${ME}`;
const REVIEWED_BY = `is:pr is:open reviewed-by:${ME}`;
const MAX_ISSUES = 120;

function gql(query, vars = {}) {
  const args = ["api", "graphql", "-f", `query=${query}`];
  for (const [k, v] of Object.entries(vars)) if (v != null) args.push(typeof v === "number" ? "-F" : "-f", `${k}=${v}`);
  return JSON.parse(execFileSync("gh", args, { encoding: "utf8", maxBuffer: 256 << 20 })).data;
}

const PR = `
  number title state isDraft url createdAt updatedAt mergedAt closedAt
  author { login } baseRefName headRefOid additions deletions changedFiles
  labels(first: 10) { nodes { name } }
  reviewRequests(first: 10) { nodes { requestedReviewer { ... on User { login } ... on Team { name } } } }
  assignees(first: 5) { nodes { login } }
  reviews(last: 30) { nodes { author { login } state submittedAt commit { oid } } }
  commits(last: 1) { totalCount nodes { commit { oid committedDate statusCheckRollup { state } } } }
  comments { totalCount }`;
const ISSUE = `number title state createdAt updatedAt author { login } labels(first: 10) { nodes { name } } comments { totalCount }`;
// Description and recent comments only in the per-Issue lookup: inside 100-result searches it times out (HTTP 504).
const ISSUE_FULL = `number title state createdAt updatedAt bodyHTML author { login } labels(first: 10) { nodes { name } }
  comments(last: 15) { totalCount nodes { author { login } bodyHTML createdAt } }`;

function search(q, limit = 100) {
  const out = [];
  let after = null;
  while (out.length < limit) {
    const d = gql(
      `query($q: String!, $after: String) { search(query: $q, type: ISSUE_ADVANCED, first: 100, after: $after) {
        pageInfo { hasNextPage endCursor }
        nodes { __typename ... on PullRequest { ${PR} } ... on Issue { ${ISSUE} } } } }`,
      { q: `repo:python/cpython ${q}`, after },
    );
    out.push(...d.search.nodes);
    if (!d.search.pageInfo.hasNextPage) break;
    after = d.search.pageInfo.endCursor;
  }
  return out.slice(0, limit);
}

const prs = new Map();
const issues = new Map();
const flat = (pr) => ({
  ...pr,
  author: pr.author?.login ?? "ghost",
  labels: pr.labels.nodes.map((l) => l.name),
  reviewRequests: pr.reviewRequests.nodes.map((n) => n.requestedReviewer?.login ?? n.requestedReviewer?.name).filter(Boolean),
  assignees: pr.assignees.nodes.map((a) => a.login),
  reviews: pr.reviews.nodes.map((r) => ({ author: r.author?.login ?? "ghost", state: r.state, at: r.submittedAt, commit: r.commit?.oid })),
  commitCount: pr.commits.totalCount,
  headDate: pr.commits.nodes[0]?.commit.committedDate,
  ci: pr.commits.nodes[0]?.commit.statusCheckRollup?.state ?? null,
  comments: pr.comments.totalCount,
});
const addNode = (n) => {
  if (n.__typename === "PullRequest") prs.set(n.number, flat(n));
  else if (n.__typename === "Issue") issues.set(n.number, { ...n, author: n.author?.login ?? "ghost", labels: n.labels.nodes.map((l) => l.name), comments: n.comments.totalCount,
    thread: n.comments.nodes?.map((c) => ({ author: c.author?.login ?? "ghost", html: c.bodyHTML, at: c.createdAt })) });
};
const issueOf = (title) => +(/^(?:\[[\d.]+\]\s*)?gh-(\d+)/i.exec(title)?.[1] ?? 0) || null;

console.log(`snapshot as ${ME}`);
const filters = FILTERS.map((f) => {
  const nodes = search(f.q, 60);
  nodes.forEach(addNode);
  console.log(`  filter "${f.name}": ${nodes.length}`);
  return { ...f, matches: nodes.map((n) => ({ kind: n.__typename === "Issue" ? "issue" : "pr", number: n.number })) };
});
const reviewRequests = search(REVIEW_REQUESTS, 50);
reviewRequests.forEach(addNode);
const reviewedBy = search(REVIEWED_BY, 50);
reviewedBy.forEach(addNode);
console.log(`  review requests: ${reviewRequests.length}, reviewed-by: ${reviewedBy.length}`);

// Every Issue reached by a matched PR, a matched issue, or a pin; then the full PR tree of each.
const wanted = new Set(PINNED);
for (const n of issues.keys()) wanted.add(n);
for (const pr of prs.values()) { const i = issueOf(pr.title); if (i) wanted.add(i); }
const issueNums = [...wanted].slice(0, MAX_ISSUES);
for (let i = 0; i < issueNums.length; i += 10) {
  const chunk = issueNums.slice(i, i + 10);
  const d = gql(`query { repository(owner: "python", name: "cpython") { ${chunk.map((n) => `i${n}: issueOrPullRequest(number: ${n}) { __typename ... on Issue { ${ISSUE_FULL} } }`).join("\n")} } }`);
  for (const n of chunk) { const node = d.repository[`i${n}`]; if (node?.__typename === "Issue") addNode(node); }
}
for (const n of issueNums) {
  if (!issues.has(n)) continue;
  const tree = search(`is:pr "gh-${n}" in:title`, n === 159739 ? 200 : 60);
  tree.forEach(addNode);
  console.log(`  gh-${n}: ${tree.length} PRs`);
}

// Recent commits (with merge flag) and CI finish time for open PRs: hover text on "stale" and "CI …".
// Separate batched pass; adding this to the 100-result searches makes GitHub time out.
const openNums = [...prs.values()].filter((p) => p.state === "OPEN").map((p) => p.number);
for (let i = 0; i < openNums.length; i += 10) {
  const chunk = openNums.slice(i, i + 10);
  const d = gql(`query { repository(owner: "python", name: "cpython") { ${chunk.map((n) => `p${n}: pullRequest(number: ${n}) {
    recent: commits(last: 30) { nodes { commit { oid abbreviatedOid committedDate messageHeadline parents { totalCount } } } }
    head: commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
      __typename ... on CheckRun { completedAt startedAt } ... on StatusContext { createdAt } } } } } } } }`).join(" ")} } }`);
  for (const n of chunk) {
    const r = d.repository[`p${n}`];
    if (!r) continue;
    const pr = prs.get(n);
    pr.recentCommits = r.recent.nodes.map(({ commit: c }) => ({ oid: c.oid, short: c.abbreviatedOid, at: c.committedDate, headline: c.messageHeadline, merge: c.parents.totalCount > 1 }));
    const times = (r.head.nodes[0]?.commit.statusCheckRollup?.contexts.nodes ?? []).map((x) => x.completedAt ?? x.startedAt ?? x.createdAt).filter(Boolean).sort();
    pr.ciAt = times.at(-1) ?? null;
  }
}
console.log(`  commits and CI times for ${openNums.length} open PRs`);

// Full conversation + diff for a handful of PRs, so the PR view has real content.
const pick = new Set();
const take = (list, k) => list.filter((n) => n.__typename === "PullRequest").slice(0, k).forEach((n) => pick.add(n.number));
take(search(`is:pr is:open author:${ME}`, 3), 3);
take(reviewRequests, 3);
take(reviewedBy, 2);
const tree159739 = [...prs.values()].filter((p) => issueOf(p.title) === 159739);
tree159739.filter((p) => p.state === "OPEN").slice(0, 3).forEach((p) => pick.add(p.number));
const botMerged = tree159739.find((p) => p.author === "miss-islington" && p.state === "MERGED");
const originMerged = tree159739.find((p) => p.baseRefName === "main" && p.state === "MERGED");
if (botMerged) pick.add(botMerged.number);
if (originMerged) pick.add(originMerged.number);

for (const n of EXTRA_DETAILS) {
  const d = gql(`query($n: Int!) { repository(owner: "python", name: "cpython") { pullRequest(number: $n) { ${PR} } } }`, { n });
  prs.set(n, flat(d.repository.pullRequest));
  pick.add(n);
}

const details = {};
for (const number of [...pick].slice(0, 18)) {
  const d = gql(
    `query($n: Int!) { repository(owner: "python", name: "cpython") { pullRequest(number: $n) {
      bodyHTML
      comments(first: 60) { nodes { author { login } bodyHTML createdAt } }
      reviews(first: 40) { nodes { author { login } state bodyHTML submittedAt } }
      reviewThreads(first: 60) { nodes { isResolved isOutdated path line originalLine startLine diffSide
        comments(first: 20) { nodes { author { login } bodyHTML createdAt } } } }
      files(first: 100) { nodes { path additions deletions } }
      commits(first: 100) { nodes { commit { oid abbreviatedOid messageHeadline committedDate author { name } } } } } } }`,
    { n: number },
  ).repository.pullRequest;
  let diff = execFileSync("gh", ["pr", "diff", String(number), "-R", "python/cpython"], { encoding: "utf8", maxBuffer: 256 << 20 });
  const lines = diff.split("\n");
  if (lines.length > 6000) diff = lines.slice(0, 6000).join("\n");
  const paths = d.files.nodes.map((f) => f.path);
  const attr = execFileSync("git", ["-C", REPO_DIR, "check-attr", "--stdin", "generated"], { input: paths.join("\n") + "\n", encoding: "utf8" });
  const generated = attr.split("\n").filter((l) => l.endsWith(": generated: set")).map((l) => l.slice(0, l.indexOf(": generated:")));
  details[number] = {
    bodyHTML: d.bodyHTML,
    comments: d.comments.nodes.map((c) => ({ author: c.author?.login ?? "ghost", html: c.bodyHTML, at: c.createdAt })),
    reviews: d.reviews.nodes.map((r) => ({ author: r.author?.login ?? "ghost", state: r.state, html: r.bodyHTML, at: r.submittedAt })),
    threads: d.reviewThreads.nodes.map((t) => ({
      resolved: t.isResolved, outdated: t.isOutdated, path: t.path, line: t.line ?? t.originalLine, startLine: t.startLine, side: t.diffSide,
      comments: t.comments.nodes.map((c) => ({ author: c.author?.login ?? "ghost", html: c.bodyHTML, at: c.createdAt })),
    })),
    files: d.files.nodes.map((f) => ({ ...f, generated: generated.includes(f.path) })),
    commits: d.commits.nodes.map((c) => ({ oid: c.commit.abbreviatedOid, headline: c.commit.messageHeadline, at: c.commit.committedDate, author: c.commit.author?.name })),
    diff,
    truncated: lines.length > 6000,
  };
  console.log(`  details #${number}: ${d.files.nodes.length} files, ${lines.length} diff lines`);
}

const out = {
  me: ME,
  takenAt: new Date().toISOString(),
  pinned: PINNED,
  filters,
  reviewRequests: reviewRequests.map((n) => n.number),
  issues: [...issues.values()],
  prs: [...prs.values()],
  details,
};
writeFileSync(new URL("../src/lib/proto/fixture.json", import.meta.url), JSON.stringify(out));
console.log(`wrote fixture: ${out.issues.length} issues, ${out.prs.length} PRs, ${Object.keys(details).length} with details`);
