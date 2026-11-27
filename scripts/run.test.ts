import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { attempt, gh, git, messageOf, run, stop, untracked } from "./run.ts";

const NODE = process.execPath;

/** Makes `process.exit` throw instead, and keeps what the script printed to stderr. */
function catchExit(): string[] {
  const printed: string[] = [];
  vi.spyOn(console, "error").mockImplementation((message: string) => printed.push(message));
  vi.spyOn(process, "exit").mockImplementation((code) => {
    throw new Error(`exit ${String(code)}`);
  });
  return printed;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("stop", () => {
  it("prints the message and exits with 1", () => {
    const printed = catchExit();
    expect(() => stop("Name the branch.")).toThrow("exit 1");
    expect(printed).toEqual(["Name the branch."]);
  });
});

describe("run", () => {
  it("gives the command's output without the blanks around it", () => {
    expect(run("node failed.", NODE, "-e", "console.log(' out ')")).toBe("out");
  });

  it("stops with `failed` when the command fails", () => {
    const printed = catchExit();
    expect(() => run("node failed.", NODE, "-e", "process.exit(3)")).toThrow("exit 1");
    expect(printed).toEqual(["node failed."]);
  });
});

describe("attempt", () => {
  it("gives the command's output", () => {
    expect(attempt(NODE, "-e", "console.log('out')")).toBe("out");
  });

  it("gives `undefined` when the command fails", () => {
    expect(attempt(NODE, "-e", "process.exit(3)")).toBeUndefined();
  });
});

describe("messageOf", () => {
  it("gives an Error's message", () => {
    expect(messageOf(new Error("Cargo.toml has no version."))).toBe("Cargo.toml has no version.");
  });

  it("gives anything else as text", () => {
    expect(messageOf(7)).toBe("7");
  });
});

describe("gh", () => {
  // A Windows CI runner busy with the rest of the build can take more than five seconds to start gh, so
  // this test gets a longer timeout than Vitest's default.
  it("gives gh's output", () => {
    expect(gh("--version")).toMatch(/^gh version \d/);
  }, 30_000);
});

describe("git", () => {
  it("gives git's output", () => {
    expect(git("--version")).toMatch(/^git version \S+$/);
  });

  it("drops only the line break, so a path that ends in a space keeps it", () => {
    expect(git("-c", "test.path=docs/a.md ", "config", "--get", "test.path")).toBe("docs/a.md ");
  });

  it("throws when git fails", () => {
    expect(() => git("no-such-command")).toThrow();
  });
});

describe("untracked", () => {
  let folder = "";

  afterEach(() => {
    rmSync(folder, { recursive: true, force: true });
  });

  it("lists the files git would add, and leaves out the ignored ones", () => {
    folder = mkdtempSync(join(tmpdir(), "untracked-"));
    git("init", "-q", folder);
    mkdirSync(join(folder, "docs"));
    writeFileSync(join(folder, ".gitignore"), "*.log\n");
    writeFileSync(join(folder, "docs", " leading space.md"), "");
    writeFileSync(join(folder, "build.log"), "");
    expect(untracked(folder).sort()).toEqual([".gitignore", "docs/ leading space.md"]);
  });
});
