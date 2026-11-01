// PROTOTYPE — throwaway. Core: where a link in a description or comment should go. An Issue or PR in
// the current Repository opens in place, one elsewhere on GitHub in a Peek, anything else in the browser.
import type { LinkTarget } from "./types";

const GH = /^https:\/\/github\.com\/([\w.-]+\/[\w.-]+)\/(issues|pull)\/(\d+)(?:[/?#].*)?$/;

export function linkTarget(href: string, current: string, added: string[]): LinkTarget {
  const m = GH.exec(href);
  if (!m) return { kind: "web" };
  const [, repo, type, num] = m;
  if (repo.toLowerCase() === current.toLowerCase()) return { kind: "here", type: type === "pull" ? "pr" : "issue", number: +num };
  return { kind: "peek", repo, number: +num, added: added.some((r) => r.toLowerCase() === repo.toLowerCase()) };
}
