// `just next` lists the open tickets of the earliest open milestone that has any, the release being built,
// with the ones ready for an agent first, so a session can pick its next ticket in one step.
import { execFileSync } from "node:child_process";

function gh(...args: string[]): string {
  return execFileSync("gh", args, { encoding: "utf8" });
}

interface Milestone {
  title: string;
}
interface Issue {
  number: number;
  title: string;
  labels: { name: string }[];
}

/** A milestone's version as numbers, padded to three parts. A title that isn't a version sorts last. */
function version(title: string): number[] {
  const parts = title.split(".").map(Number);
  return parts.every(Number.isInteger) ? [...parts, 0, 0].slice(0, 3) : [Infinity, Infinity, Infinity];
}
function earlier(a: Milestone, b: Milestone): number {
  const [x, y] = [version(a.title), version(b.title)];
  const at = x.findIndex((part, i) => part !== y[i]);
  return at === -1 ? 0 : Number(x[at]) - Number(y[at]);
}

const milestones = JSON.parse(
  gh("api", "repos/{owner}/{repo}/milestones?state=open&per_page=100"),
) as Milestone[];
// A milestone's open_issues counts PRs too, so ask for each one's issues until one has some.
for (const milestone of milestones.sort(earlier)) {
  const issues = JSON.parse(
    gh(
      "issue",
      "list",
      "--milestone",
      milestone.title,
      "--state",
      "open",
      "--limit",
      "200",
      "--json",
      "number,title,labels",
    ),
  ) as Issue[];
  if (issues.length === 0) {
    continue;
  }
  const ready = (issue: Issue): boolean => issue.labels.some((label) => label.name === "ready-for-agent");
  console.log(`${milestone.title}: ${String(issues.length)} open, ready for an agent first`);
  for (const issue of [...issues.filter(ready), ...issues.filter((issue) => !ready(issue))]) {
    const labels = issue.labels.map((label) => label.name).join(", ");
    console.log(`#${String(issue.number)} ${issue.title} [${labels}]`);
  }
  process.exit(0);
}
console.log("No open milestone has open tickets.");
