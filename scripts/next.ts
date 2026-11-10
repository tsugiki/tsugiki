// `just next` lists the open tickets of the earliest open milestone that has any, the release being built,
// so a session can pick its next ticket in one step. A ticket prints under the blockers it has in the
// milestone, and the ones ready for an agent come first: `tickets.ts` holds that order.
import { execFileSync } from "node:child_process";

import { byVersion } from "./pr.ts";
import { type Ticket, ticketLines } from "./tickets.ts";

function gh(...args: string[]): string {
  return execFileSync("gh", args, { encoding: "utf8" });
}

interface Milestone {
  number: number;
  title: string;
}
interface Issues {
  totalCount: number;
  nodes: {
    number: number;
    title: string;
    labels: { nodes: { name: string }[] };
    blockedBy: { nodes: { number: number; state: "OPEN" | "CLOSED" }[] };
  }[];
}

// A milestone's `issues` holds no PRs, unlike its `open_issues` count. GitHub caps each page at 100, and the
// newest issues come first, as `gh issue list` gives them. `blockedBy` can't filter by state, so closed
// blockers count toward its 100 too.
const QUERY = `query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    milestone(number: $number) {
      issues(states: OPEN, first: 100, orderBy: {field: CREATED_AT, direction: DESC}) {
        totalCount
        nodes {
          number
          title
          labels(first: 100) { nodes { name } }
          blockedBy(first: 100) { nodes { number state } }
        }
      }
    }
  }
}`;

function openIssues(milestone: Milestone): Issues {
  const reply = gh(
    "api",
    "graphql",
    "-F",
    "owner={owner}",
    "-F",
    "name={repo}",
    "-F",
    `number=${String(milestone.number)}`,
    "-f",
    `query=${QUERY}`,
  );
  return (JSON.parse(reply) as { data: { repository: { milestone: { issues: Issues } } } }).data.repository
    .milestone.issues;
}

const milestones = JSON.parse(
  gh("api", "repos/{owner}/{repo}/milestones?state=open&per_page=100"),
) as Milestone[];
for (const milestone of milestones.sort((a, b) => byVersion(a.title, b.title))) {
  const issues = openIssues(milestone);
  if (issues.totalCount === 0) {
    continue;
  }
  const tickets: Ticket[] = issues.nodes.map((issue) => ({
    number: issue.number,
    title: issue.title,
    labels: issue.labels.nodes.map((label) => label.name),
    openBlockers: issue.blockedBy.nodes.filter((blocker) => blocker.state === "OPEN").map((b) => b.number),
  }));
  const shown =
    issues.nodes.length < issues.totalCount ? `, the newest ${String(issues.nodes.length)} shown` : "";
  console.log(
    `${milestone.title}: ${String(issues.totalCount)} open${shown}, each under its blockers, ready for an agent first`,
  );
  for (const line of ticketLines(tickets)) {
    console.log(line);
  }
  process.exit(0);
}
console.log("No open milestone has open tickets.");
