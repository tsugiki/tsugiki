// `just waves` reads a milestone's `## Waves` table back, so a wave takes its tickets from the plan instead
// of a row copied from the description. `just waves` prints every wave of the release being built, `just
// waves 2` that wave, and `just waves 2 0.3.7` that wave of the milestone named. `--bodies` adds each
// ticket's title, state, labels and body, and `--json` prints the same for an agent. `waves.ts` holds the
// parsing, the arguments and the output.
import { gh, messageOf, stop } from "./run.ts";
import {
  type Milestone,
  type Ticket,
  type TicketRepository,
  currentMilestone,
  milestoneNamed,
  ticketQuery,
  ticketsIn,
  wavesArgs,
  wavesFor,
  wavesIn,
  wavesJson,
  wavesText,
} from "./waves.ts";

// Every milestone, a page of 100, with the open issues that decide which one is being built. `gh` fills only
// a cursor named `$endCursor`, so the connection is paged that way.
const MILESTONES = `query($owner: String!, $name: String!, $endCursor: String) {
  repository(owner: $owner, name: $name) {
    milestones(first: 100, states: [OPEN, CLOSED], after: $endCursor) {
      nodes {
        title
        state
        description
        issues(states: OPEN) { totalCount }
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}`;

interface MilestoneNode {
  title: string;
  state: "OPEN" | "CLOSED";
  /** Null until someone writes a description. */
  description: string | null;
  issues: { totalCount: number };
}

const args = wavesArgs(process.argv.slice(2));
if (args === undefined) {
  stop("just waves takes a wave number, a milestone version, --bodies and --json.");
}

const pages = JSON.parse(
  gh(
    "api",
    "graphql",
    "--paginate",
    "--slurp",
    "-F",
    "owner={owner}",
    "-F",
    "name={repo}",
    "-f",
    `query=${MILESTONES}`,
  ),
) as { data: { repository: { milestones: { nodes: MilestoneNode[] } } } }[];
const milestones: Milestone[] = pages.flatMap((page) =>
  page.data.repository.milestones.nodes.map((node) => ({
    title: node.title,
    description: node.description ?? "",
    open: node.state === "OPEN",
    openIssues: node.issues.totalCount,
  })),
);

const milestone =
  args.version === undefined ? currentMilestone(milestones) : milestoneNamed(milestones, args.version);
if (milestone === undefined) {
  stop(
    args.version === undefined
      ? "No open milestone has open tickets."
      : `No milestone is titled ${args.version}.`,
  );
}

const waves = (() => {
  try {
    return wavesIn(milestone.description);
  } catch (cause) {
    stop(`${milestone.title}: ${messageOf(cause)}`);
  }
})();

const picked = (() => {
  try {
    return wavesFor(waves, args.wave);
  } catch (cause) {
    stop(`${milestone.title}: ${messageOf(cause)}`);
  }
})();

const tickets = new Map<number, Ticket>();
if (args.bodies) {
  const numbers = picked.flatMap((wave) => wave.tickets);
  const answer = JSON.parse(
    gh("api", "graphql", "-F", "owner={owner}", "-F", "name={repo}", "-f", `query=${ticketQuery(numbers)}`),
  ) as { data: { repository: TicketRepository } };
  const found = (() => {
    try {
      return ticketsIn(answer.data.repository);
    } catch (cause) {
      stop(messageOf(cause));
    }
  })();
  for (const ticket of found) {
    tickets.set(ticket.number, ticket);
  }
}

console.log(
  args.json
    ? JSON.stringify(wavesJson(milestone.title, picked, tickets), null, 2)
    : wavesText(picked, tickets),
);
