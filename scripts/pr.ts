// The decisions behind `just ship` and `just next`, kept apart from the git and GitHub calls in ship.ts and
// next.ts so that tests can reach them.

/** The options `just ship` was given. */
export interface ShipArgs {
  check: boolean;
  release: boolean;
  bodyFile: string | undefined;
}

/** `just ship [--check] [--release] [body.md]`, or `undefined` for anything else, such as a mistyped flag. */
export function shipArgs(args: string[]): ShipArgs | undefined {
  const [bodyFile, ...extra] = args.filter((arg) => arg !== "--check" && arg !== "--release");
  if (bodyFile?.startsWith("-") || extra.length > 0) {
    return undefined;
  }
  return { check: args.includes("--check"), release: args.includes("--release"), bodyFile };
}

/** The reason `just ship` won't push from `branch`, or `undefined` when it may. */
export function branchRefusal(branch: string, release: boolean): string | undefined {
  if (branch === "" || branch === "main") {
    return "Ship from the PR's branch, not main.";
  }
  const isRelease = branch.startsWith("release/");
  if (isRelease && !release) {
    return (
      "A push to a release/ branch rehearses the release on CI, smoke test included: ask the user, then " +
      "run `just ship --release`."
    );
  }
  if (!isRelease && release) {
    return `\`--release\` ships only a release/ branch, and this is ${branch}.`;
  }
  return undefined;
}

/** The lines of a failed `just check`'s log worth printing: its last 30, then the recipes that failed. */
export function failureSummary(log: string): string {
  const lines = log.trimEnd().split(/\r?\n/);
  const failed = lines.filter((line) => /recipe `[\w-]+` failed/.test(line));
  return [...lines.slice(-30), "", ...failed].join("\n");
}

/** A milestone's version as numbers, padded to three parts. A title that isn't a version sorts last. */
function version(title: string): number[] {
  const parts = title.split(".").map(Number);
  return parts.every(Number.isInteger) ? [...parts, 0, 0].slice(0, 3) : [Infinity, Infinity, Infinity];
}

/** Sorts milestone titles by version, earliest first, for `Array.prototype.sort`. */
export function byVersion(a: string, b: string): number {
  const [x, y] = [version(a), version(b)];
  const at = x.findIndex((part, i) => part !== y[i]);
  return at === -1 ? 0 : Number(x[at]) - Number(y[at]);
}
