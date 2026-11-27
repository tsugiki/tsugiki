// The reading behind `just waves`: the `## Waves` table of a milestone description, the arguments the command
// takes, and what it prints. The milestone's description holds the plan, so this reads the plan back instead
// of copying a row by hand.

import { byVersion } from "./releasing.ts";

/** One row of the `## Waves` table: the wave's number and the tickets it holds. */
export interface Wave {
  number: number;
  tickets: number[];
}

/** A milestone as the command chooses it. `openIssues` counts its open issues, not its pull requests. */
export interface Milestone {
  title: string;
  description: string;
  open: boolean;
  openIssues: number;
}

/** What `just waves` was asked for. A field is `undefined` when the command wasn't given it. */
export interface WavesArgs {
  wave: number | undefined;
  version: string | undefined;
  bodies: boolean;
  json: boolean;
}

/** A ticket as `--bodies` and `--json` show it. */
export interface Ticket {
  number: number;
  title: string;
  state: string;
  labels: string[];
  body: string;
}

/** A ticket as `ticketQuery` asks for it, before GitHub's label connection is unfolded. */
interface RawTicket {
  number: number;
  title: string;
  state: string;
  body: string;
  labels: { nodes: { name: string }[] };
}

/** `ticketQuery`'s answer under `data.repository`: one entry per ticket alias, null for a missing ticket. */
export type TicketRepository = Record<string, RawTicket | null>;

const FLAGS = ["--bodies", "--json"];
const NUMBER = /^\d+$/;

/** The arguments `just waves` takes, or `undefined` for arguments it doesn't. */
export function wavesArgs(args: string[]): WavesArgs | undefined {
  const named = args.filter((arg) => !FLAGS.includes(arg));
  if (named.some((arg) => arg.startsWith("-"))) {
    return undefined;
  }
  const numbers = named.filter((arg) => NUMBER.test(arg));
  const versions = named.filter((arg) => !NUMBER.test(arg));
  const [waveText] = numbers;
  const [version] = versions;
  if (numbers.length > 1 || versions.length > 1 || (waveText !== undefined && Number(waveText) < 1)) {
    return undefined;
  }
  return {
    wave: waveText === undefined ? undefined : Number(waveText),
    version,
    bodies: args.includes("--bodies"),
    json: args.includes("--json"),
  };
}

/** The milestone with no version named: the earliest open one with open tickets, as `just next` picks it. */
export function currentMilestone(milestones: Milestone[]): Milestone | undefined {
  return milestones
    .filter((milestone) => milestone.open && milestone.openIssues > 0)
    .sort((a, b) => byVersion(a.title, b.title))[0];
}

/** The milestone titled `version`, open or closed, so a released plan stays readable. */
export function milestoneNamed(milestones: Milestone[], version: string): Milestone | undefined {
  return milestones.find((milestone) => milestone.title === version);
}

const SEPARATOR = /^:?-+:?$/;
const HEADING = "## Waves";

/** A table row's cells, trimmed, without the pipes at its ends. */
function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

/**
 * The waves in `description`'s `## Waves` table, which runs to the next `## ` heading. Throws an `Error`
 * naming what is wrong with the table: a missing section, a column, a wave number or a ticket.
 */
export function wavesIn(description: string): Wave[] {
  const lines = description.split(/\r?\n/);
  const heading = lines.findIndex((line) => line.trim() === HEADING);
  if (heading === -1) {
    throw new Error('The description has no "## Waves" section.');
  }
  const after = lines.slice(heading + 1);
  const next = after.findIndex((line) => line.trim().startsWith("## "));
  const rows = (next === -1 ? after : after.slice(0, next))
    .filter((line) => line.trimStart().startsWith("|"))
    .map(cells);
  const [header] = rows;
  if (header === undefined || !header.includes("Wave") || !header.includes("Tickets")) {
    throw new Error('The "## Waves" section has no table with a Wave and a Tickets column.');
  }
  const waveAt = header.indexOf("Wave");
  const ticketsAt = header.indexOf("Tickets");
  const waves: Wave[] = [];
  const where = new Map<number, number>();
  for (const row of rows.slice(1).filter((row) => !row.every((cell) => SEPARATOR.test(cell)))) {
    const waveCell = row[waveAt] ?? "";
    const ticketsCell = row[ticketsAt] ?? "";
    if (!/^[1-9]\d*$/.test(waveCell)) {
      throw new Error(`The "## Waves" table has "${waveCell}" where a wave number goes.`);
    }
    const number = Number(waveCell);
    if (waves.some((wave) => wave.number === number)) {
      throw new Error(`The "## Waves" table lists wave ${waveCell} twice.`);
    }
    const tickets = ticketsCell
      .split(/[+,]/)
      .map((ticket) => ticket.trim())
      .filter((ticket) => ticket !== "");
    if (tickets.length === 0) {
      throw new Error(`Wave ${waveCell} lists no tickets.`);
    }
    const numbers: number[] = [];
    for (const ticket of tickets) {
      if (!/^#\d+$/.test(ticket)) {
        throw new Error(`Wave ${waveCell} lists "${ticket}" where a #N ticket goes.`);
      }
      const ticketNumber = Number(ticket.slice(1));
      const earlier = where.get(ticketNumber);
      if (earlier === number) {
        throw new Error(`Wave ${waveCell} lists #${String(ticketNumber)} twice.`);
      }
      if (earlier !== undefined) {
        throw new Error(`#${String(ticketNumber)} is in wave ${String(earlier)} and wave ${waveCell}.`);
      }
      where.set(ticketNumber, number);
      numbers.push(ticketNumber);
    }
    waves.push({ number, tickets: numbers });
  }
  if (waves.length === 0) {
    throw new Error('The "## Waves" table has no waves.');
  }
  return waves;
}

/**
 * The waves `just waves` prints: all of them, or the one numbered `wave`. Throws an `Error` naming the waves
 * it has when it has no such wave.
 */
export function wavesFor(waves: Wave[], wave: number | undefined): Wave[] {
  if (wave === undefined) {
    return waves;
  }
  const picked = waves.filter((candidate) => candidate.number === wave);
  if (picked.length === 0) {
    throw new Error(
      `It has waves ${waves.map((candidate) => String(candidate.number)).join(", ")}; ` +
        `not wave ${String(wave)}.`,
    );
  }
  return picked;
}

/** The GraphQL query for `numbers`' tickets, one alias each, with what `--bodies` and `--json` show. */
export function ticketQuery(numbers: number[]): string {
  const aliases = numbers
    .map((number) => `    ticket${String(number)}: issue(number: ${String(number)}) { ...TicketFields }`)
    .join("\n");
  return `query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
${aliases}
  }
}
fragment TicketFields on Issue {
  number
  title
  state
  body
  labels(first: 100) { nodes { name } }
}`;
}

/** The tickets in `ticketQuery`'s answer, or an `Error` naming the ones GitHub didn't return. */
export function ticketsIn(repository: TicketRepository): Ticket[] {
  const missing: string[] = [];
  const tickets: Ticket[] = [];
  for (const [alias, raw] of Object.entries(repository)) {
    if (raw === null) {
      missing.push(`#${alias.replace("ticket", "")}`);
    } else {
      const { number, title, state, body, labels } = raw;
      tickets.push({ number, title, state, body, labels: labels.nodes.map((label) => label.name) });
    }
  }
  if (missing.length > 0) {
    throw new Error(`GitHub returned no ticket for ${missing.join(", ")}.`);
  }
  return tickets;
}

/** One ticket's block under its wave, for `--bodies`. */
function ticketText(ticket: Ticket): string {
  const labels = ticket.labels.length === 0 ? "none" : ticket.labels.join(", ");
  return `#${String(ticket.number)} ${ticket.title}\nstate: ${ticket.state}, labels: ${labels}\n\n${ticket.body}`;
}

/** What `just waves` prints: a line per wave, and with `tickets` each ticket's block under its wave. */
export function wavesText(waves: Wave[], tickets: Map<number, Ticket> = new Map()): string {
  const lines: string[] = [];
  for (const wave of waves) {
    lines.push(
      `Wave ${String(wave.number)}: ${wave.tickets.map((number) => `#${String(number)}`).join(", ")}`,
    );
    for (const number of wave.tickets) {
      const ticket = tickets.get(number);
      if (ticket !== undefined) {
        lines.push("", ticketText(ticket));
      }
    }
  }
  return lines.join("\n");
}

/** The JSON `just waves --json` prints; `found` holds the tickets only when `--bodies` asked for them. */
export function wavesJson(
  milestone: string,
  waves: Wave[],
  found: Map<number, Ticket> = new Map(),
): { milestone: string; waves: { wave: number; tickets: (Ticket | { number: number })[] }[] } {
  return {
    milestone,
    waves: waves.map(({ number, tickets }) => ({
      wave: number,
      tickets: tickets.map((ticket) => found.get(ticket) ?? { number: ticket }),
    })),
  };
}
