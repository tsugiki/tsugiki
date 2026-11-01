// PROTOTYPE — throwaway. Git side of data fetching and processing (the real app does this in Rust).
// Uses a throwaway bare mirror, `.proto-mirror` (PROTOTYPE — wipe me), that borrows objects from the
// user's CPython clone through `objects/info/alternates`: the clone is only read, never written
// (ADR 0002's --reference bootstrap). PR heads are fetched from GitHub into refs/gh-rev/pr/N.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const REMOTE = "https://github.com/python/cpython.git";

export function openMirror(dir, cloneDir) {
  const git = (args, input) =>
    execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", input, maxBuffer: 256 << 20, stdio: ["pipe", "pipe", "pipe"] });
  const tryGit = (args, input) => { try { return git(args, input); } catch { return null; } };

  if (!existsSync(resolve(dir, "HEAD"))) {
    mkdirSync(dir, { recursive: true });
    execFileSync("git", ["init", "--bare", "-q", dir]);
    const common = execFileSync("git", ["-C", cloneDir, "rev-parse", "--path-format=absolute", "--git-common-dir"], { encoding: "utf8" }).trim();
    writeFileSync(resolve(dir, "objects/info/alternates"), resolve(common, "objects") + "\n");
    git(["remote", "add", "origin", REMOTE]);
  }

  return {
    /** Fetch target branches and PR heads in one go; objects already in the clone aren't downloaded. */
    fetch(branches, prNumbers) {
      const specs = [...branches.map((b) => `+refs/heads/${b}:refs/heads/${b}`), ...prNumbers.map((n) => `+refs/pull/${n}/head:refs/gh-rev/pr/${n}`)];
      for (let i = 0; i < specs.length; i += 50) git(["fetch", "-q", "--no-tags", "origin", ...specs.slice(i, i + 50)]);
    },
    has: (oid) => tryGit(["cat-file", "-e", `${oid}^{commit}`]) != null,
    /** A reviewed commit may be gone from the branch after a force-push; GitHub still serves it by SHA. */
    fetchCommit: (oid) => tryGit(["fetch", "-q", "--no-tags", "origin", oid]) != null,
    /** Paths marked `generated` by .gitattributes on the given branch. */
    generated(paths, branch) {
      if (!paths.length) return new Set();
      const out = git(["check-attr", `--source=refs/heads/${branch}`, "--stdin", "generated"], paths.join("\n") + "\n");
      return new Set(out.split("\n").filter((l) => l.endsWith(": generated: set")).map((l) => l.slice(0, l.indexOf(": generated:"))));
    },
    /**
     * Has the PR's own diff changed between commit `from` and commit `to`, ignoring Generated files?
     * Each side is diffed against its merge-base with the target branch (what GitHub shows as the PR
     * diff), so merges from the target branch drop out, and patch-id ignores line-number shifts.
     * `--verbatim` keeps whitespace: in Python a dedent changes meaning. Files whose patches differ only
     * in whitespace (they'd match under `--stable`) are listed separately in `whitespaceOnly`.
     */
    diffChange(from, to, branch) {
      const base = (c) => git(["merge-base", `refs/heads/${branch}`, c]).trim();
      const [bFrom, bTo] = [base(from), base(to)];
      const files = (b, c) => git(["diff", "--name-only", "--no-renames", b, c]).split("\n").filter(Boolean);
      const all = [...new Set([...files(bFrom, from), ...files(bTo, to)])];
      const gen = this.generated(all, branch);
      const ids = (b, c, f) => {
        const patch = git(["diff", "--no-color", "--no-ext-diff", "--no-renames", b, c, "--", f]);
        const id = (mode) => git(["patch-id", mode], `commit 0000000000000000000000000000000000000000\n${patch}`).split(" ")[0];
        return patch ? { verbatim: id("--verbatim"), stable: id("--stable") } : { verbatim: "", stable: "" };
      };
      const differs = [], whitespace = [];
      for (const f of all) {
        const [a, b] = [ids(bFrom, from, f), ids(bTo, to, f)];
        if (a.verbatim !== b.verbatim) differs.push(f);
        if (a.verbatim !== b.verbatim && a.stable === b.stable) whitespace.push(f);
      }
      const changed = differs.filter((f) => !gen.has(f));
      return { same: changed.length === 0, changed, whitespaceOnly: whitespace.filter((f) => !gen.has(f)), generatedChanged: differs.filter((f) => gen.has(f)) };
    },
  };
}
