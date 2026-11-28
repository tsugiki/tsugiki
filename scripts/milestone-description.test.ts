import { resolve } from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { run } from "./run.ts";

vi.mock("./run.ts", () => ({
  run: vi.fn(),
  stop: (message: string): never => {
    throw new Error(message);
  },
}));

beforeEach(() => {
  vi.resetModules();
  vi.mocked(run).mockReset();
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
});

test.each([undefined, "0", "18/other"])("rejects milestone %s before calling GitHub", async (milestone) => {
  vi.stubEnv("milestone", milestone);
  vi.stubEnv("description_file", "draft.md");

  await expect(import("./milestone-description.ts")).rejects.toThrow("Pass a positive milestone number");
  expect(run).not.toHaveBeenCalled();
});

test("requires a description file", async () => {
  vi.stubEnv("milestone", "18");
  vi.stubEnv("description_file", undefined);

  await expect(import("./milestone-description.ts")).rejects.toThrow("Pass a description file");
  expect(run).not.toHaveBeenCalled();
});

test("patches only the description from the given file and prints the milestone title", async () => {
  const file = "draft with spaces.md";
  vi.stubEnv("milestone", "42");
  vi.stubEnv("description_file", file);
  vi.mocked(run).mockReturnValue("Next release");
  const output = vi.spyOn(console, "log").mockImplementation(() => {});

  await import("./milestone-description.ts");

  expect(run).toHaveBeenCalledExactlyOnceWith(
    "Could not update the milestone description.",
    "gh",
    "api",
    "--hostname",
    "github.com",
    "-X",
    "PATCH",
    "repos/tsugiki/tsugiki/milestones/42",
    "-F",
    `description=@${resolve(file)}`,
    "--jq",
    ".title",
  );
  expect(output).toHaveBeenCalledExactlyOnceWith("Next release");
});
