// The rules behind `just prose-check`, which lists lines to reread against /stop-slop. A match is a reason to
// look: words like "every" are often right. To correct a rule, edit its entry in `RULES`.
import { docLines } from "./doc-lines.ts";

/** A pattern worth a second look, and why. */
export interface Rule {
  name: string;
  pattern: RegExp;
  reason: string;
}

/** A line that matched a rule. */
export interface Finding {
  at: number;
  rule: string;
  text: string;
}

/** Words ending in -ly that aren't adverbs, or that we use as plain words. */
const NOT_ADVERBS = new Set([
  "ally",
  "apply",
  "assembly",
  "comply",
  "early",
  "family",
  "fly",
  "imply",
  "july",
  "multiply",
  "only",
  "rely",
  "reply",
  "supply",
]);

function adverbs(): RegExp {
  const skipped = [...NOT_ADVERBS].join("|");
  return new RegExp(`\\b(?!(?:${skipped})\\b)[a-z]+ly\\b`, "i");
}

export const RULES: Rule[] = [
  { name: "adverb", pattern: adverbs(), reason: "Cut it, or say what it hedges." },
  { name: "filler", pattern: /\b(?:just|very|quite|somehow)\b/i, reason: "It adds no meaning." },
  { name: "em-dash", pattern: /[—–]/, reason: "Use a full stop or a comma." },
  {
    name: "wh-opener",
    pattern: /(?:^|^[-*+]\s+|[.!?:]\s+)(?:What|When|Where|Why|Which|Who|How)\b/,
    reason: "Name the thing. A docstring says what it documents.",
  },
  {
    name: "lazy-extreme",
    pattern: /\b(?:always|never|every|everything)\b/i,
    reason: "Keep it only when it's exact.",
  },
  {
    name: "throat-clearing",
    pattern:
      /\b(?:here's (?:what|this|that|why|the)|it turns out|it's worth noting|the (?:truth|reality) is|at the end of the day|when it comes to|at its core|let me|this matters because)\b/i,
    reason: "Cut the announcement and state the point.",
  },
  {
    name: "contrast",
    pattern:
      /\b(?:(?:isn't|aren't|wasn't|not)\b[^.]*,\s*(?:it's|but)\b|not just\b|not only\b|rather than\b)/i,
    reason: "State the second half directly.",
  },
  {
    name: "mention",
    pattern: /(?:^|[\s(])@[a-z0-9]/i,
    reason: "GitHub notifies whoever has this login. Use backticks or drop the @.",
  },
];

/** The lines of `source` that break a rule, once per rule. `only` limits this to those line numbers. */
export function findings(path: string, source: string, only?: Set<number>): Finding[] {
  return docLines(path, source)
    .filter(({ at }) => only === undefined || only.has(at))
    .flatMap(({ at, text }) => {
      const prose = text.replace(/`[^`]*`/g, "");
      const broken = RULES.filter((rule) => rule.pattern.test(prose));
      return broken.map((rule) => ({ at, rule: rule.name, text }));
    });
}

/** The lines each file gained, from a `git diff --unified=0`. */
export function addedLines(diff: string): Map<string, Set<number>> {
  const added = new Map<string, Set<number>>();
  let file: string | undefined;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ")) {
      file = line.startsWith("+++ b/") ? line.slice("+++ b/".length) : undefined;
      continue;
    }
    const hunk = /^@@ -\S+ \+(\d+)(?:,(\d+))? @@/.exec(line);
    if (hunk !== null && file !== undefined) {
      const first = Number(hunk[1]);
      const count = hunk[2] === undefined ? 1 : Number(hunk[2]);
      if (count > 0) {
        const lines = added.get(file) ?? new Set<number>();
        for (let at = first; at < first + count; at += 1) {
          lines.add(at);
        }
        added.set(file, lines);
      }
    }
  }
  return added;
}
