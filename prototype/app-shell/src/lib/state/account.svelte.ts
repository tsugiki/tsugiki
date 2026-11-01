// PROTOTYPE — throwaway. The token and what it allows in each Repository. There's no GitHub here:
// SIM stands in for what GitHub would do with each kind of token (?token=, or the select on the Account
// page), and a write "fails" where GitHub would refuse it. A refused write makes the Repository Read-only.
import { canWrite, type Token } from "$lib/core/tokens";
import { value, setValue, secret } from "./settings.svelte";
import { status } from "./status.svelte";
import { app, say, boardOf, REPOS } from "./app.svelte";

export const SIM: [string, string][] = [
  ["gh", "gh CLI token (repo, read:org, gist, workflow)"],
  ["classic", "Classic token, public_repo"],
  ["read", "Fine-grained, read-only"],
  ["review", "Fine-grained, reviews on python's repositories"],
  ["pending", "Fine-grained, reviews on python's, waiting for python's approval"],
  ["unsure", "Fine-grained, reviews on python's, “Not sure” chosen"],
];
const GH_SCOPES = ["gist", "read:org", "repo", "workflow"];

export const account = $state({
  sim: "gh",
  /** What GitHub said, per token: Repositories where a write was refused, or went through. */
  refused: {} as Record<string, string[]>,
  wrote: {} as Record<string, string[]>,
  /** The guide the user last opened GitHub's form from: a fine-grained token pasted next is assumed to be that one. */
  template: null as null | { allows: "read" | "write"; owner: string },
});

export const me = () => boardOf(REPOS[0]).me;

export function token(): Token {
  if (value("auth.source") !== "token") return { kind: "gh", scopes: GH_SCOPES };
  if (secret.token.startsWith("ghp_")) return { kind: "classic", scopes: ["public_repo"] };
  return { kind: "fine", allows: value("auth.fine_allows") as "read" | "write" | "unsure", owner: (value("auth.fine_owner") as string) || me() };
}
/** Is a token there to use at all? */
export const hasToken = () => value("auth.source") !== "token" || /^(ghp_|github_pat_)/.test(secret.token);
const fp = () => `${value("auth.source")}:${secret.token}`;

/** What GitHub itself would do with a write to `repo` (the simulation's truth). */
function githubAllows(repo: string) {
  const t = token();
  if (t.kind !== "fine") return t.scopes.includes("repo") || t.scopes.includes("public_repo");
  return (account.sim === "review" || account.sim === "unsure") && repo.startsWith("python/");
}

export type Access = "write" | "read" | "unknown";
export function access(repo: string | null = app.repo): Access {
  if (!repo || status.scenario === "offline" || status.scenario === "token" || !hasToken()) return "read";
  if (account.refused[fp()]?.includes(repo)) return "read";
  const c = canWrite(token(), repo);
  return c === true ? "write" : c === false ? "read" : account.wrote[fp()]?.includes(repo) ? "write" : "unknown";
}

/** Why `repo` is Read-only, in a few words. */
export function whyReadOnly(repo: string | null = app.repo): string {
  const t = token();
  if (status.scenario === "offline") return "Offline";
  if (status.scenario === "token") return "the token has expired";
  if (!hasToken()) return "no token yet";
  if (account.refused[fp()]?.includes(repo ?? "")) return `GitHub refused a write with this token${t.kind === "fine" && t.allows === "write" ? " (does it still wait for approval?)" : ""}`;
  if (t.kind === "fine" && t.allows === "read") return "your token is read-only";
  if (t.kind === "fine") return `your token writes only to ${t.owner}'s repositories`;
  return "your token has no write scope";
}

/** Call before writing to GitHub. False: don't write, keep the text; the Repository is Read-only now. */
export function tryWrite(repo: string | null = app.repo): boolean {
  if (!repo || access(repo) === "read") return false;
  const k = fp();
  if (githubAllows(repo)) { account.wrote[k] = [...(account.wrote[k] ?? []), repo]; return true; }
  account.refused[k] = [...(account.refused[k] ?? []), repo];
  say(`GitHub refused: this token can't write to ${repo}. What you wrote is kept; ${repo} is Read-only now.`);
  return false;
}

/** Repositories where GitHub refused a write with the current token. */
export const refusedNow = () => account.refused[fp()] ?? [];
/** After the token changed on GitHub (approved, more access): find out again on the next write. */
export function forgetRefusals() { delete account.refused[fp()]; say("gh-rev will try writing again and see what GitHub says"); }

/** The prototype's stand-in for pasting different tokens. */
export function simulate(key: string) {
  account.sim = key;
  if (key === "gh") { setValue("auth.source", "gh"); secret.token = ""; return; }
  setValue("auth.source", "token");
  if (key === "classic") { secret.token = "ghp_PROTOTYPEclassic000000000000000000"; return; }
  secret.token = `github_pat_PROTOTYPE${key}_00000000000`;
  setValue("auth.fine_allows", key === "read" ? "read" : key === "unsure" ? "unsure" : "write");
  setValue("auth.fine_owner", key === "read" ? "" : "python");
}
