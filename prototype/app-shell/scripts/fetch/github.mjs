// PROTOTYPE — throwaway. GitHub side of data fetching (the real app does this in Rust): read-only
// GraphQL through `gh api graphql` with the user's gh token, normalized into the snapshot's shapes
// (src/lib/core/types.ts). Never writes to GitHub.
import { execFileSync } from "node:child_process";

const REPO = `repository(owner: "python", name: "cpython")`;

function gql(query, vars = {}) {
  const args = ["api", "graphql", "-f", `query=${query}`];
  for (const [k, v] of Object.entries(vars)) if (v != null) args.push(typeof v === "number" ? "-F" : "-f", `${k}=${v}`);
  return JSON.parse(execFileSync("gh", args, { encoding: "utf8", maxBuffer: 256 << 20 })).data;
}

export const whoami = () => execFileSync("gh", ["api", "user", "--jq", ".login"], { encoding: "utf8" }).trim();

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

const login = (a) => a?.login ?? "ghost";
const post = (c) => ({ author: login(c.author), html: c.bodyHTML, at: c.createdAt });

function toPr(pr) {
  return {
    ...pr,
    author: login(pr.author),
    labels: pr.labels.nodes.map((l) => l.name),
    reviewRequests: pr.reviewRequests.nodes.map((n) => n.requestedReviewer?.login ?? n.requestedReviewer?.name).filter(Boolean),
    assignees: pr.assignees.nodes.map((a) => a.login),
    reviews: pr.reviews.nodes.map((r) => ({ author: login(r.author), state: r.state, at: r.submittedAt, commit: r.commit?.oid })),
    commitCount: pr.commits.totalCount,
    headDate: pr.commits.nodes[0]?.commit.committedDate,
    ci: pr.commits.nodes[0]?.commit.statusCheckRollup?.state ?? null,
    comments: pr.comments.totalCount,
  };
}
function toIssue(i) {
  return { ...i, author: login(i.author), labels: i.labels.nodes.map((l) => l.name), comments: i.comments.totalCount, thread: i.comments.nodes?.map(post) };
}

/** Advanced search (OR and parentheses work). Returns PRs and Issues in result order. */
export function search(q, limit = 100) {
  const out = [];
  let after = null;
  while (out.length < limit) {
    const d = gql(
      `query($q: String!, $after: String) { search(query: $q, type: ISSUE_ADVANCED, first: 100, after: $after) {
        pageInfo { hasNextPage endCursor }
        nodes { __typename ... on PullRequest { ${PR} } ... on Issue { ${ISSUE} } } } }`,
      { q: `repo:python/cpython ${q}`, after },
    );
    for (const n of d.search.nodes)
      if (n.__typename === "PullRequest") out.push({ kind: "pr", number: n.number, value: toPr(n) });
      else if (n.__typename === "Issue") out.push({ kind: "issue", number: n.number, value: toIssue(n) });
    if (!d.search.pageInfo.hasNextPage) break;
    after = d.search.pageInfo.endCursor;
  }
  return out.slice(0, limit);
}

/** Aliased lookups, 10 per query: heavier fields in bigger batches make GitHub time out. */
function batched(nums, field) {
  const out = new Map();
  for (let i = 0; i < nums.length; i += 10) {
    const chunk = nums.slice(i, i + 10);
    const d = gql(`query { ${REPO} { ${chunk.map((n) => `n${n}: ${field(n)}`).join("\n")} } }`);
    for (const n of chunk) if (d.repository[`n${n}`]) out.set(n, d.repository[`n${n}`]);
  }
  return out;
}

/** Issues with description and recent comments; numbers that turn out to be PRs are skipped. */
export function issues(nums) {
  const got = batched(nums, (n) => `issueOrPullRequest(number: ${n}) { __typename ... on Issue { ${ISSUE_FULL} } }`);
  return [...got.values()].filter((x) => x.__typename === "Issue").map(toIssue);
}

export const pr = (n) => toPr(gql(`query($n: Int!) { ${REPO} { pullRequest(number: $n) { ${PR} } } }`, { n }).repository.pullRequest);

/** Open PRs: the last 30 commits (with a merge flag) and when CI last finished on the head. */
export function prActivity(nums) {
  const got = batched(nums, (n) => `pullRequest(number: ${n}) {
    recent: commits(last: 30) { nodes { commit { oid abbreviatedOid committedDate messageHeadline parents { totalCount } } } }
    head: commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
      __typename ... on CheckRun { completedAt startedAt } ... on StatusContext { createdAt } } } } } } } }`);
  const out = new Map();
  for (const [n, r] of got) {
    const times = (r.head.nodes[0]?.commit.statusCheckRollup?.contexts.nodes ?? []).map((x) => x.completedAt ?? x.startedAt ?? x.createdAt).filter(Boolean).sort();
    out.set(n, {
      recentCommits: r.recent.nodes.map(({ commit: c }) => ({ oid: c.oid, short: c.abbreviatedOid, at: c.committedDate, headline: c.messageHeadline, merge: c.parents.totalCount > 1 })),
      ciAt: times.at(-1) ?? null,
    });
  }
  return out;
}

/** Full conversation and diff of one PR, for the PR view. Files carry no Generated flag yet (git side). */
export function prDetail(n, maxDiffLines = 6000) {
  const d = gql(
    `query($n: Int!) { ${REPO} { pullRequest(number: $n) {
      bodyHTML
      comments(first: 60) { nodes { author { login } bodyHTML createdAt } }
      reviews(first: 40) { nodes { author { login } state bodyHTML submittedAt } }
      reviewThreads(first: 60) { nodes { isResolved isOutdated path line originalLine startLine diffSide
        comments(first: 20) { nodes { author { login } bodyHTML createdAt } } } }
      files(first: 100) { nodes { path additions deletions } }
      commits(first: 100) { nodes { commit { oid abbreviatedOid messageHeadline committedDate author { name } } } } } } }`,
    { n },
  ).repository.pullRequest;
  const lines = execFileSync("gh", ["pr", "diff", String(n), "-R", "python/cpython"], { encoding: "utf8", maxBuffer: 256 << 20 }).split("\n");
  return {
    bodyHTML: d.bodyHTML,
    comments: d.comments.nodes.map(post),
    reviews: d.reviews.nodes.map((r) => ({ author: login(r.author), state: r.state, html: r.bodyHTML, at: r.submittedAt })),
    threads: d.reviewThreads.nodes.map((t) => ({
      resolved: t.isResolved, outdated: t.isOutdated, path: t.path, line: t.line ?? t.originalLine, startLine: t.startLine, side: t.diffSide,
      comments: t.comments.nodes.map(post),
    })),
    files: d.files.nodes,
    commits: d.commits.nodes.map((c) => ({ oid: c.commit.abbreviatedOid, headline: c.commit.messageHeadline, at: c.commit.committedDate, author: c.commit.author?.name })),
    diff: lines.slice(0, maxDiffLines).join("\n"),
    truncated: lines.length > maxDiffLines,
  };
}
