// `just tree 393` prints #393 and every open ticket it blocks, itself or through other tickets, as a tree of
// their blockers. `just tree 393 398` prints one tree per ticket named, and `just tree` one per open ticket
// with no open blocker, and one per cycle no other ticket blocks. `just tree --up 914` prints #914 and every
// open ticket that blocks it, itself or through other tickets, with each ticket's open blockers under it.
// `tree.ts` holds the layout.
import { execFileSync } from "node:child_process";

import { stop } from "./run.ts";
import { blockerLines, QUERY, ticketsFromPages, treeLines } from "./tree.ts";

const up = process.argv.includes("--up");
const args = process.argv.slice(2).filter((arg) => arg !== "--up");
const bad = args.filter((arg) => !/^\d+$/.test(arg));
if (bad.length > 0) {
  stop(`just tree takes ticket numbers and --up, not ${bad.join(", ")}`);
}
if (up && args.length === 0) {
  stop("just tree --up takes the ticket numbers whose blockers it lists");
}
const named = args.map(Number);

const reply = execFileSync(
  "gh",
  [
    "api",
    "graphql",
    "--paginate",
    "--slurp",
    "-F",
    "owner={owner}",
    "-F",
    "name={repo}",
    "-f",
    `query=${QUERY}`,
  ],
  { encoding: "utf8", maxBuffer: 2 ** 28 },
);
const { tickets, warning } = ticketsFromPages(reply);
if (warning !== undefined) {
  console.error(warning);
}
const open = new Set(tickets.map((ticket) => ticket.number));
const closed = named.filter((number) => !open.has(number));
if (closed.length > 0) {
  stop(`Not an open issue: ${closed.map((n) => `#${String(n)}`).join(", ")}`);
}
const lines = up ? blockerLines(tickets, named) : treeLines(tickets, named);
console.log(lines.length === 0 ? "No open tickets." : lines.join("\n"));
