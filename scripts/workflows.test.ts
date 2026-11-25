// GitHub Actions can't share an `if:` or a version between jobs or workflows, so these tests check that the
// copies agree, with each other and with the scripts.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { releasePage } from "./release-notes.ts";
import { releaseBranch } from "./releasing.ts";
import { EVERY_JOB } from "./scope.ts";

const read = (workflow: string): string[] =>
  readFileSync(`../.github/workflows/${workflow}`, "utf8").split(/\r?\n/);

const CI = read("ci.yml");
const RELEASE = read("release.yml");
const ADVISORIES = read("advisories.yml");

/** The number of spaces `line` starts with. */
const indentOf = (line: string): number => line.length - line.trimStart().length;

/**
 * The value of the `if:` at `lines[at]`, with a folded `>-` value's lines joined by spaces, or `undefined`
 * when that line isn't an `if:`.
 */
function ifAt(lines: readonly string[], at: number): string | undefined {
  const line = lines[at] ?? "";
  const value = /^\s*(?:- )?if: (.*)$/.exec(line)?.[1];
  if (value !== ">-") {
    return value;
  }
  const folded: string[] = [];
  for (const next of lines.slice(at + 1)) {
    if (indentOf(next) <= indentOf(line)) break;
    folded.push(next.trim());
  }
  return folded.join(" ");
}

/** Each job of a workflow, by its key, with the lines under that key. */
function jobsOf(lines: readonly string[]): Map<string, string[]> {
  const jobs = new Map<string, string[]>();
  let job: string[] | undefined;
  for (const line of lines.slice(lines.indexOf("jobs:") + 1)) {
    const key = /^ {2}([\w-]+):$/.exec(line)?.[1];
    if (key !== undefined) {
      job = [];
      jobs.set(key, job);
    } else {
      job?.push(line);
    }
  }
  return jobs;
}

/** The `if:` of a job itself, not of one of its steps, or `undefined` when it has none. */
function jobIf(lines: readonly string[]): string | undefined {
  const at = lines.findIndex((line) => /^ {4}if: /.test(line));
  return at === -1 ? undefined : ifAt(lines, at);
}

/** The `if:` of the step named `name`. */
function stepIf(lines: readonly string[], name: string): string | undefined {
  const at = lines.findIndex((line) => line.trim() === `- name: ${name}`);
  return ifAt(lines, at + 1);
}

/** A clause of a CI job's `if:` that asks whether `changes` listed a job of scope.ts. */
const LISTED = /contains\(fromJSON\(needs\.changes\.outputs\.jobs\), '(\w+)'\)/g;

const CI_JOBS = jobsOf(CI);

describe("CI's jobs", () => {
  const passed = CI_JOBS.get("ci-passed") ?? [];
  const needs = passed.flatMap((line) => /^ {6}- ([\w-]+)$/.exec(line)?.[1] ?? []);

  /** The jobs of scope.ts each CI job reads, by `CI passed`'s case table. */
  const table = new Map(
    passed.flatMap((line) => {
      const [, names = "", reads = ""] = /^\s+([\w| -]+)\) reads='(.+)' ;;$/.exec(line) ?? [];
      const listed = [...reads.matchAll(/"(\w+)"/g)].map(([, name]) => name);
      return names.split("|").map((name) => [name.trim(), listed] as const);
    }),
  );

  /** The jobs of scope.ts a CI job reads, by its own `if:`, or `always` for a job with none. */
  const reads = (job: string): string[] => {
    const condition = jobIf(CI_JOBS.get(job) ?? []);
    return condition === undefined
      ? ["always"]
      : [...condition.matchAll(LISTED)].flatMap(([, name]) => name ?? []);
  };

  it("all count towards CI passed", () => {
    expect(needs.toSorted()).toEqual([...CI_JOBS.keys()].filter((job) => job !== "ci-passed").toSorted());
  });

  it("each skip on what CI passed's case table says they read", () => {
    expect(table.size).toBeGreaterThan(0);
    for (const job of needs) {
      expect(table.get(job), job).toEqual(reads(job));
    }
  });

  it("ask only whether `changes` lists what they read", () => {
    for (const job of needs) {
      const left = (jobIf(CI_JOBS.get(job) ?? []) ?? "").replaceAll(LISTED, "");
      expect(left, job).toMatch(/^\$\{\{[\s|]*\}\}$|^$/);
    }
  });

  it("read each job scope.ts names, and nothing else", () => {
    const named = new Set(needs.flatMap(reads).filter((read) => read !== "always"));
    expect([...named].toSorted()).toEqual(EVERY_JOB.toSorted());
  });
});

describe("the release branch rule", () => {
  const ciRule = stepIf(
    CI_JOBS.get("release-check") ?? [],
    "A new version or Release workflow comes from a release/ branch",
  );
  const releaseRule = jobIf(jobsOf(RELEASE).get("check") ?? []);

  it("in CI is the complement of the Release workflow's", () => {
    const pr = /^\$\{\{ github\.event_name == 'pull_request' && !\((.+)\) \}\}$/.exec(ciRule ?? "")?.[1];
    const rehearsal = /^\$\{\{ github\.event_name == 'push' \|\| \((.+)\) \}\}$/.exec(releaseRule ?? "")?.[1];
    expect(pr).toBeDefined();
    expect(pr).toBe(rehearsal);
  });

  it("names the release branches the scripts make", () => {
    expect(releaseRule).toContain(`startsWith(github.head_ref, '${releaseBranch("")}')`);
  });
});

describe("pinned tools", () => {
  const pins = (lines: readonly string[], tool: string): string[] =>
    lines.flatMap((line) => [...line.matchAll(new RegExp(`${tool}@\\S+`, "g"))].map(([pin]) => pin));

  it("install the same cargo-deny in CI and the advisories check", () => {
    expect(pins(CI, "cargo-deny")).toHaveLength(1);
    expect(pins(ADVISORIES, "cargo-deny")).toEqual(pins(CI, "cargo-deny"));
  });

  it("name each coverage job's tools once in CI, and have both jobs install them", () => {
    for (const tool of ["cargo-llvm-cov", "nextest"]) {
      expect(pins(CI, tool), tool).toHaveLength(1);
    }
    for (const job of ["rust-coverage", "macos"]) {
      const tools = (CI_JOBS.get(job) ?? []).filter((line) => line.trim().startsWith("tool: "));
      expect(
        tools.map((line) => line.trim()),
        job,
      ).toEqual(["tool: ${{ env.LLVM_COV_TOOL }}, ${{ env.NEXTEST_TOOL }}"]);
    }
  });
});

describe("tsugiki-core's benchmarks", () => {
  /** The features any `[[bench]]` of tsugiki-core requires, which Cargo skips the benchmark without. */
  const required = (): string[] => {
    const manifest = readFileSync("../crates/core/Cargo.toml", "utf8");
    return manifest
      .split("[[bench]]")
      .slice(1)
      .flatMap((bench) => {
        const list = /^required-features = \[(.*)\]$/m.exec(bench.split(/^\[/m)[0] ?? "")?.[1];
        return [...(list ?? "").matchAll(/"([\w-]+)"/g)].map(([, feature]) => feature ?? "");
      });
  };

  /** The `--features` that `command` turns on. */
  const features = (command: string): string[] =>
    [...command.matchAll(/--features (\S+)/g)].flatMap(([, list]) => (list ?? "").split(","));

  it("require a feature only on the store benchmark, which `testing::Reads` serves", () => {
    expect(required()).toEqual(["testing"]);
  });

  it("are built on CodSpeed and by `npm run bench` with each feature they require", () => {
    const build = read("bench.yml").find((line) => line.includes("cargo codspeed build")) ?? "";
    const scripts = (JSON.parse(readFileSync("../package.json", "utf8")) as { scripts: { bench: string } })
      .scripts;
    for (const command of [build, scripts.bench]) {
      expect(features(command), command).toEqual(expect.arrayContaining(required()));
    }
  });
});

describe("the Release workflow", () => {
  it("checks each download with the command the release page gives", () => {
    const at = RELEASE.findIndex((line) => line.trim().startsWith("gh attestation verify "));
    const lines = [RELEASE[at] ?? ""];
    while (lines.at(-1)?.endsWith("\\")) {
      lines.push(RELEASE[at + lines.length] ?? "");
    }
    const command = lines.map((line) => line.trim().replace(/\s*\\$/, "")).join(" ");
    const repo = RELEASE.find((line) => line.startsWith("  REPO: "))?.slice("  REPO: ".length);
    expect(repo).toBe("tsugiki/tsugiki");
    const page = releasePage("- Notes.").split("\n");
    const shown = command.replace('"$file"', "<file>").replaceAll(/"\$REPO([^"]*)"/g, `${repo}$1`);
    expect(page).toContain(shown);
  });
});
