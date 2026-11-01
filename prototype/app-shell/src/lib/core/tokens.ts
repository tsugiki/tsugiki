// PROTOTYPE — throwaway. GitHub tokens: what each kind lets gh-rev (and anyone holding it) do, in plain
// words, and the guides for making a narrower one. Pure: no DOM, no runes.

/** What gh-rev knows about the token. gh's and classic tokens report their scopes (X-OAuth-Scopes);
 *  a fine-grained one reports nothing, so its `allows` is what the user said, or learned from a refused write. */
export type Token =
  | { kind: "gh"; scopes: string[] }
  | { kind: "classic"; scopes: string[] }
  | { kind: "fine"; allows: "read" | "write" | "unsure"; owner: string };

/** One line of "What this token lets gh-rev do": ok, no (can't), or warn (can, and gh-rev doesn't need it). */
export interface Can { mark: "ok" | "no" | "warn" | "unknown"; text: string }

const writesPublic = (scopes: string[]) => scopes.includes("repo") || scopes.includes("public_repo");

/** Can this token write reviews and comments to `repo`? null: gh-rev can't tell until it tries. */
export function canWrite(t: Token, repo: string): boolean | null {
  if (t.kind !== "fine") return writesPublic(t.scopes);
  if (t.allows === "unsure") return null;
  return t.allows === "write" && repo.split("/")[0].toLowerCase() === t.owner.toLowerCase();
}

/** `refused`: Repositories where GitHub refused a write with this token, whatever it was expected to allow. */
export function capabilities(t: Token, repos: string[], refused: string[] = []): Can[] {
  const out: Can[] = [{ mark: "ok", text: "Read public repositories: everything gh-rev shows" }];
  if (t.kind === "fine") {
    if (t.allows === "unsure") out.push({ mark: "unknown", text: "Reviews and comments: not known yet. GitHub doesn't tell apps what a fine-grained token allows; gh-rev finds out on the first write, and keeps what you were sending." });
    else for (const r of repos) out.push(refused.includes(r) ? { mark: "no", text: `Reviews and comments in ${r}: Read-only, GitHub refused a write` } : canWrite(t, r)
      ? { mark: "ok", text: `Reviews and comments in ${r}` }
      : { mark: "no", text: `Reviews and comments in ${r}: Read-only${t.allows === "write" ? ` (the token's owner is ${t.owner})` : ""}` });
    out.push({ mark: "ok", text: "Can't push, merge, or see private repositories" });
    return out;
  }
  if (refused.length) out.push({ mark: "no", text: `Reviews and comments in ${refused.join(", ")}: Read-only, GitHub refused a write (an organization can block tokens like this one)` });
  out.push(writesPublic(t.scopes) ? { mark: "ok", text: `Reviews and comments in ${refused.length ? "the other Repositories" : "every Repository"}` } : { mark: "no", text: "Reviews and comments: Read-only everywhere" });
  const extra = [
    t.scopes.includes("repo") ? "push to and merge in any repository you can push to, python/cpython included" : t.scopes.includes("public_repo") ? "push to and merge in any public repository you can push to, python/cpython included" : "",
    t.scopes.includes("repo") ? "read and write your private repositories" : "",
    t.scopes.includes("workflow") ? "change GitHub Actions workflows" : "",
    t.scopes.includes("gist") ? "read and write your gists" : "",
    t.scopes.some((s) => s.includes("org")) ? "read your organizations and teams" : "",
  ].filter(Boolean);
  if (extra.length) out.push({ mark: "warn", text: `Also, though gh-rev never uses it: ${extra.join("; ")}` });
  return out;
}

/** The guides on the Account page: each makes a narrower token than gh's. */
export interface Guide { key: string; title: string; pitch: string; url: (owner: string) => string; steps: (owner: string) => string[]; can: string[]; cannot: string[]; note?: (owner: string, isMe: boolean) => string }

const q = (p: Record<string, string>) => new URLSearchParams(p).toString();
export const GUIDES: Guide[] = [
  {
    key: "read", title: "Read-only", pitch: "To try gh-rev. Nothing it holds can change anything on GitHub.",
    url: () => `https://github.com/settings/personal-access-tokens/new?${q({ name: "gh-rev (read-only)", description: "gh-rev: reads public repositories. No permissions.", expires_in: "90" })}`,
    steps: () => [
      "Open GitHub's form below; the name and a 90-day expiry are filled in.",
      "Leave Resource owner as yourself and Repository access as “Public repositories”.",
      "Leave every permission at “No access”, then Generate token.",
      "Copy the token (github_pat_…) and paste it above.",
    ],
    can: ["Show everything: Workbench, Issues, PRs, diffs, CI, Local marks"],
    cannot: ["Comment, review or resolve threads: write them in gh-rev, then “Copy and open on GitHub”", "Push, merge, or see private repositories"],
  },
  {
    key: "review", title: "Reviews on one owner's repositories", pitch: "Comments and reviews from gh-rev, with no way to push or merge.",
    url: (owner) => `https://github.com/settings/personal-access-tokens/new?${q({ name: "gh-rev (reviews)", description: "gh-rev: comments and reviews. Cannot push or merge.", target_name: owner, expires_in: "90", pull_requests: "write", issues: "write", contents: "read" })}`,
    steps: (owner) => [
      `Open GitHub's form below; it is filled in for ${owner}: Pull requests and Issues “Read and write”, Contents “Read-only”.`,
      `Under Repository access choose “Only select repositories” and pick the ${owner} repositories you added (GitHub can't fill that in).`,
      "Generate token, copy it (github_pat_…) and paste it above.",
    ],
    can: ["Comment, review, reply, resolve threads, suggest changes"],
    cannot: ["Push or merge", "Write to another owner's repositories: they stay Read-only"],
    note: (owner, isMe) => isMe ? "" : `If ${owner} requires approval for fine-grained tokens, the token waits for an owner of ${owner} to approve it. Until then GitHub lets it read only, and gh-rev stays Read-only.`,
  },
  {
    key: "classic", title: "Classic, public repositories", pitch: "Works for every owner with no approval, but can push.",
    url: () => `https://github.com/settings/tokens/new?${q({ scopes: "public_repo", description: "gh-rev" })}`,
    steps: () => [
      "Open GitHub's form below; only “public_repo” is ticked.",
      "Pick an expiry, Generate token, copy it (ghp_…) and paste it above.",
    ],
    can: ["Everything gh-rev does, in every Repository"],
    cannot: ["Read private repositories, gists or workflows"],
    note: () => "It can still push to and merge in any public repository you can push to, python/cpython included. Prefer the fine-grained one when you can.",
  },
];
