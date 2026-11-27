import { describe, expect, it } from "vitest";

import {
  type Milestone,
  type Ticket,
  currentMilestone,
  milestoneNamed,
  ticketQuery,
  ticketsIn,
  wavesArgs,
  wavesIn,
  wavesFor,
  wavesJson,
  wavesText,
} from "./waves.ts";

const DESCRIPTION = `## Lanes

| Lane | Order |
|---|---|
| Search | #464 → #465 |

## Waves

| Wave | Tickets | Start | End | Wall-clock |
|---|---|---|---|---|
| 1 | #464, #1082, #1073 | 11-26 07:12 | 11-26 09:42 | 150 |
| 2 | #465, #1077 | | | |
| 3 | #472 | | | |

## Wave log

| Wave | Tickets |
|---|---|
| 9 | #999 |
`;

const TICKET: Ticket = {
  number: 465,
  title: "Search the Inbox",
  state: "OPEN",
  labels: ["area:sync", "ready-for-agent"],
  body: "The Inbox parses a query.",
};

describe("wavesArgs", () => {
  it.each([
    [[], { wave: undefined, version: undefined, bodies: false, json: false }],
    [["2"], { wave: 2, version: undefined, bodies: false, json: false }],
    [["2", "0.3.7"], { wave: 2, version: "0.3.7", bodies: false, json: false }],
    [["0.3.7", "--json", "2"], { wave: 2, version: "0.3.7", bodies: false, json: true }],
    [["--bodies", "--json"], { wave: undefined, version: undefined, bodies: true, json: true }],
    [["--bodies", "--json", "0.3.7"], { wave: undefined, version: "0.3.7", bodies: true, json: true }],
  ])("reads %j", (args, expected) => {
    expect(wavesArgs(args)).toEqual(expected);
  });

  it.each([[["1", "2"]], [["0.3.7", "0.4.0"]], [["0"]], [["00"]], [["--body"]], [["1", "2", "0.3.7"]]])(
    "refuses %j",
    (args) => {
      expect(wavesArgs(args)).toBeUndefined();
    },
  );
});

describe("currentMilestone", () => {
  const milestone = (title: string, open: boolean, openIssues: number): Milestone => ({
    title,
    description: "",
    open,
    openIssues,
  });

  it("picks the earliest open milestone with open tickets", () => {
    const milestones = [
      milestone("0.4.0", true, 3),
      milestone("0.3.7", true, 15),
      milestone("0.3.8", true, 0),
      milestone("0.3.6", false, 4),
    ];
    expect(currentMilestone(milestones)?.title).toBe("0.3.7");
  });

  it("finds none when every open milestone is empty", () => {
    expect(currentMilestone([milestone("0.3.8", true, 0), milestone("0.3.7", false, 2)])).toBeUndefined();
  });
});

describe("milestoneNamed", () => {
  it("finds a milestone by title, open or closed", () => {
    const milestones: Milestone[] = [
      { title: "0.3.7", description: "", open: false, openIssues: 0 },
      { title: "0.4.0", description: "", open: true, openIssues: 3 },
    ];
    expect(milestoneNamed(milestones, "0.3.7")?.open).toBe(false);
    expect(milestoneNamed(milestones, "0.4.0")?.open).toBe(true);
    expect(milestoneNamed(milestones, "0.3.8")).toBeUndefined();
  });
});

describe("wavesIn", () => {
  it("reads the table, and stops at the next heading", () => {
    expect(wavesIn(DESCRIPTION)).toEqual([
      { number: 1, tickets: [464, 1082, 1073] },
      { number: 2, tickets: [465, 1077] },
      { number: 3, tickets: [472] },
    ]);
  });

  it("reads a table that ends the description", () => {
    expect(wavesIn("## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | #1 |\n")).toEqual([
      { number: 1, tickets: [1] },
    ]);
  });

  it("reads a bundled row, which a past plan wrote with plus signs", () => {
    expect(wavesIn("## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | #1, #2 + #3 |\n")).toEqual([
      { number: 1, tickets: [1, 2, 3] },
    ]);
  });

  it("finds the columns by their names", () => {
    expect(wavesIn("## Waves\n\n| Tickets | Wave |\n|---|---|\n| #1 | 4 |\n")).toEqual([
      { number: 4, tickets: [1] },
    ]);
  });

  it.each([
    ["a description without the section", "## Lanes\n\n| Wave | Tickets |\n", 'no "## Waves" section'],
    ["an empty section", "## Waves\n\n## Wave log\n", "no table"],
    ["a table without a Wave column", "## Waves\n\n| Round | Tickets |\n|---|---|\n| 1 | #1 |\n", "no table"],
    [
      "a table without a Tickets column",
      "## Waves\n\n| Wave | Start |\n|---|---|\n| 1 | today |\n",
      "no table",
    ],
    ["a table without waves", "## Waves\n\n| Wave | Tickets |\n|---|---|\n", "no waves"],
    ["a row without a wave number", "## Waves\n\n| Wave | Tickets |\n|---|---|\n| first | #1 |\n", "first"],
    [
      "a row too short for its wave column",
      "## Waves\n\n| Start | Wave | Tickets |\n|---|---|---|\n| today |\n",
      "where a wave number",
    ],
    [
      "a row too short for its tickets column",
      "## Waves\n\n| Wave | Start | Tickets |\n|---|---|---|\n| 1 | today |\n",
      "Wave 1 lists no tickets",
    ],
    ["a repeated wave", "## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | #1 |\n| 1 | #2 |\n", "twice"],
    ["a wave without tickets", "## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 |  |\n", "no tickets"],
    ["a ticket without #", "## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | 465 |\n", "where a #N ticket"],
    [
      "a ticket in two waves",
      "## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | #1 |\n| 2 | #1 |\n",
      "#1 is in wave 1 and wave 2",
    ],
    [
      "a ticket twice in one wave",
      "## Waves\n\n| Wave | Tickets |\n|---|---|\n| 1 | #1, #1 |\n",
      "lists #1 twice",
    ],
  ])("refuses %s", (_case, description, message) => {
    expect(() => wavesIn(description)).toThrow(message);
  });
});

describe("wavesFor", () => {
  const WAVES = [
    { number: 1, tickets: [1] },
    { number: 2, tickets: [2, 3] },
  ];

  it("gives every wave when none is named", () => {
    expect(wavesFor(WAVES, undefined)).toEqual(WAVES);
  });

  it("gives the wave named", () => {
    expect(wavesFor(WAVES, 2)).toEqual([{ number: 2, tickets: [2, 3] }]);
  });

  it("names the waves it has when the one named is absent", () => {
    expect(() => wavesFor(WAVES, 3)).toThrow("It has waves 1, 2; not wave 3.");
  });
});

describe("ticketQuery", () => {
  it("asks for one aliased ticket per number", () => {
    const query = ticketQuery([465, 1077]);
    expect(query).toContain("ticket465: issue(number: 465)");
    expect(query).toContain("ticket1077: issue(number: 1077)");
    expect(query).toContain("labels(first: 100)");
  });
});

describe("ticketsIn", () => {
  it("unfolds each ticket's labels", () => {
    expect(
      ticketsIn({
        ticket465: {
          number: 465,
          title: "Search the Inbox",
          state: "OPEN",
          body: "The Inbox parses a query.",
          labels: { nodes: [{ name: "area:sync" }, { name: "ready-for-agent" }] },
        },
      }),
    ).toEqual([TICKET]);
  });

  it("refuses an answer that left a ticket out", () => {
    expect(() => ticketsIn({ ticket465: null, ticket999: null })).toThrow(
      "GitHub returned no ticket for #465, #999.",
    );
  });
});

describe("wavesText", () => {
  it("prints a line per wave", () => {
    expect(wavesText(wavesIn(DESCRIPTION))).toBe(
      "Wave 1: #464, #1082, #1073\nWave 2: #465, #1077\nWave 3: #472",
    );
  });

  it("prints each ticket's block under its wave when given the tickets", () => {
    const waves = [{ number: 2, tickets: [465, 472] }];
    const tickets = new Map([
      [465, TICKET],
      [472, { ...TICKET, number: 472, title: "Save a search", labels: [], body: "" }],
    ]);
    expect(wavesText(waves, tickets)).toBe(
      "Wave 2: #465, #472\n" +
        "\n" +
        "#465 Search the Inbox\n" +
        "state: OPEN, labels: area:sync, ready-for-agent\n" +
        "\n" +
        "The Inbox parses a query.\n" +
        "\n" +
        "#472 Save a search\n" +
        "state: OPEN, labels: none\n" +
        "\n",
    );
  });
});

describe("wavesJson", () => {
  it("gives numbers alone without the tickets", () => {
    expect(wavesJson("0.3.7", [{ number: 2, tickets: [465] }])).toEqual({
      milestone: "0.3.7",
      waves: [{ wave: 2, tickets: [{ number: 465 }] }],
    });
  });

  it("gives each whole ticket when given them", () => {
    expect(wavesJson("0.3.7", [{ number: 2, tickets: [465] }], new Map([[465, TICKET]]))).toEqual({
      milestone: "0.3.7",
      waves: [{ wave: 2, tickets: [TICKET] }],
    });
  });
});
