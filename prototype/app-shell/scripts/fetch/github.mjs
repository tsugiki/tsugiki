// PROTOTYPE — throwaway. GitHub side of data fetching (the real app does this in Rust): read-only
// GraphQL through `gh api graphql` with the user's gh token, normalized into the snapshot's shapes
// (src/lib/core/types.ts). Never writes to GitHub.
import { execFileSync } from "node:child_process";

function gql(query, vars = {}) {
  const args = ["api", "graphql", "-f", `query=${query}`];
  for (const [k, v] of Object.entries(vars)) if (v != null) args.push(typeof v === "number" ? "-F" : "-f", `${k}=${v}`);
  const run = () => JSON.parse(execFileSync("gh", args, { encoding: "utf8", maxBuffer: 256 << 20, stdio: ["ignore", "pipe", "pipe"] })).data;
  // GitHub sometimes answers a heavy query with an empty body (a timeout); one retry usually does it.
  try { return run(); } catch { return run(); }
}

export const whoami = () => execFileSync("gh", ["api", "user", "--jq", ".login"], { encoding: "utf8" }).trim();

const PR = `
  number title state isDraft url createdAt updatedAt mergedAt closedAt
  author { login } baseRefName headRefName headRefOid additions deletions changedFiles
  labels(first: 10) { nodes { name } }
  reviewRequests(first: 10) { nodes { requestedReviewer { ... on User { login } ... on Team { name } } } }
  assignees(first: 5) { nodes { login } }
  reviews(last: 30) { nodes { author { login } state submittedAt commit { oid } } }
  commits(last: 1) { totalCount nodes { commit { oid committedDate statusCheckRollup { state } } } }
  closingIssuesReferences(first: 10) { nodes { number } }
  comments { totalCount }`;
const ISSUE = `number title state url createdAt updatedAt author { login } labels(first: 10) { nodes { name } } assignees(first: 5) { nodes { login } } comments { totalCount }`;
// Description, recent comments and participants only in per-Issue lookups: inside 100-result searches
// they time out (HTTP 504).
const ISSUE_FULL = `${ISSUE} bodyHTML participants(first: 50) { nodes { login } }
  closedByPullRequestsReferences(first: 20, includeClosedPrs: true) { nodes { number } }
  thread: comments(last: 15) { nodes { author { login } bodyHTML createdAt } }`;

const login = (a) => a?.login ?? "ghost";
const post = (c) => ({ author: login(c.author), html: c.bodyHTML, at: c.createdAt });

function toPr(pr) {
  const { closingIssuesReferences, ...rest } = pr;
  return {
    ...rest,
    author: login(pr.author),
    labels: pr.labels.nodes.map((l) => l.name),
    reviewRequests: pr.reviewRequests.nodes.map((n) => n.requestedReviewer?.login ?? n.requestedReviewer?.name).filter(Boolean),
    assignees: pr.assignees.nodes.map((a) => a.login),
    reviews: pr.reviews.nodes.map((r) => ({ author: login(r.author), state: r.state, at: r.submittedAt, commit: r.commit?.oid })),
    commitCount: pr.commits.totalCount,
    headDate: pr.commits.nodes[0]?.commit.committedDate,
    ci: pr.commits.nodes[0]?.commit.statusCheckRollup?.state ?? null,
    comments: pr.comments.totalCount,
    // GitHub's own "Fixes #N" links; it ignores them on PRs that don't target the default branch.
    closes: closingIssuesReferences.nodes.map((n) => n.number),
  };
}
function toIssue(i) {
  const { participants, closedByPullRequestsReferences, thread, ...rest } = i;
  return {
    ...rest, author: login(i.author), labels: i.labels.nodes.map((l) => l.name), assignees: i.assignees.nodes.map((a) => a.login),
    comments: i.comments.totalCount,
    ...(thread && { thread: thread.nodes.map(post), participants: participants.nodes.map((p) => p.login), closedBy: closedByPullRequestsReferences.nodes.map((n) => n.number) }),
  };
}

/** Read-only access to one repository, "owner/name". */
export function repo(full) {
  const [owner, name] = full.split("/");
  const REPO = `repository(owner: "${owner}", name: "${name}")`;

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

  return {
    full,
    /** Default branch, and whether the user administers it (a Maintained repository). */
    info() {
      const r = gql(`query { ${REPO} { defaultBranchRef { name } viewerPermission } }`).repository;
      return { defaultBranch: r.defaultBranchRef.name, maintained: r.viewerPermission === "ADMIN" };
    },

    /** Advanced search (OR and parentheses work). Returns PRs and Issues in result order. */
    search(q, limit = 100) {
      const out = [];
      let after = null;
      while (out.length < limit) {
        const d = gql(
          `query($q: String!, $after: String) { search(query: $q, type: ISSUE_ADVANCED, first: 50, after: $after) {
            pageInfo { hasNextPage endCursor }
            nodes { __typename ... on PullRequest { ${PR} } ... on Issue { ${ISSUE} } } } }`,
          { q: `repo:${full} ${q}`, after },
        );
        for (const n of d.search.nodes)
          if (n.__typename === "PullRequest") out.push({ kind: "pr", number: n.number, value: toPr(n) });
          else if (n.__typename === "Issue") out.push({ kind: "issue", number: n.number, value: toIssue(n) });
        if (!d.search.pageInfo.hasNextPage) break;
        after = d.search.pageInfo.endCursor;
      }
      return out.slice(0, limit);
    },

    /** Issues with description, recent comments, participants and the PRs GitHub links to them. */
    issues(nums) {
      const got = batched(nums, (n) => `issueOrPullRequest(number: ${n}) { __typename ... on Issue { ${ISSUE_FULL} } }`);
      return [...got.values()].filter((x) => x.__typename === "Issue").map(toIssue);
    },

    prs: (nums) => [...batched(nums, (n) => `pullRequest(number: ${n}) { ${PR} }`).values()].map(toPr),

    /** Descriptions as plain text, for "Closes #N" on Stacked PRs (GitHub doesn't link those). */
    bodies: (nums) => new Map([...batched(nums, (n) => `pullRequest(number: ${n}) { bodyText }`)].map(([n, r]) => [n, r.bodyText])),

    /** Open PRs: recent commits (merge flag), CI finish time, recent comments and participants. */
    prActivity(nums) {
      const got = batched(nums, (n) => `pullRequest(number: ${n}) {
        recent: commits(last: 30) { nodes { commit { oid abbreviatedOid committedDate messageHeadline parents { totalCount } } } }
        head: commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
          __typename ... on CheckRun { completedAt startedAt } ... on StatusContext { createdAt } } } } } } }
        posts: comments(last: 10) { nodes { author { login } createdAt bodyText } }
        participants(first: 50) { nodes { login } } }`);
      const out = new Map();
      for (const [n, r] of got) {
        const times = (r.head.nodes[0]?.commit.statusCheckRollup?.contexts.nodes ?? []).map((x) => x.completedAt ?? x.startedAt ?? x.createdAt).filter(Boolean).sort();
        out.set(n, {
          recentCommits: r.recent.nodes.map(({ commit: c }) => ({ oid: c.oid, short: c.abbreviatedOid, at: c.committedDate, headline: c.messageHeadline, merge: c.parents.totalCount > 1 })),
          ciAt: times.at(-1) ?? null,
          posts: r.posts.nodes.map((c) => ({ author: login(c.author), at: c.createdAt, text: c.bodyText.slice(0, 2000) })),
          participants: r.participants.nodes.map((p) => p.login),
        });
      }
      return out;
    },

    /** Full conversation and diff of one PR, for the PR view. Files carry no Generated flag yet (git side). */
    prDetail(n, maxDiffLines = 6000) {
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
      const lines = execFileSync("gh", ["pr", "diff", String(n), "-R", full], { encoding: "utf8", maxBuffer: 256 << 20 }).split("\n");
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
    },
  };
}

/**
 * Light cards for Issues and PRs anywhere on GitHub, for Peek: [{ repo: "owner/name", number }].
 * Returns "owner/name#N" -> card.
 */
export function peeks(refs) {
  const out = new Map();
  for (let i = 0; i < refs.length; i += 10) {
    const chunk = refs.slice(i, i + 10);
    let d;
    try { d = gql(`query { ${chunk.map((r, k) => {
      const [o, n] = r.repo.split("/");
      return `r${k}: repository(owner: "${o}", name: "${n}") { issueOrPullRequest(number: ${r.number}) {
        __typename ... on Issue { title state url createdAt author { login } bodyHTML comments(last: 3) { totalCount nodes { author { login } bodyHTML createdAt } } }
        ... on PullRequest { title state isDraft url createdAt author { login } bodyHTML comments(last: 3) { totalCount nodes { author { login } bodyHTML createdAt } } } } }`;
    }).join("\n")} }`); } catch { continue; } // a private or deleted repository fails the whole batch
    chunk.forEach((r, k) => {
      const x = d[`r${k}`]?.issueOrPullRequest;
      if (!x) return;
      out.set(`${r.repo}#${r.number}`, {
        repo: r.repo, number: r.number, kind: x.__typename === "Issue" ? "issue" : "pr", title: x.title, state: x.isDraft ? "DRAFT" : x.state,
        url: x.url, createdAt: x.createdAt, author: login(x.author), bodyHTML: x.bodyHTML, comments: x.comments.totalCount, last: x.comments.nodes.map(post),
      });
    });
  }
  return out;
}
