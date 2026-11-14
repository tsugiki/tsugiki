import { expect, it } from "vitest";
import { problemWords, statusWords, stderrTail, versionWords } from "./server-words";

it("says what went wrong with a server in the words of each kind of problem", () => {
  expect(
    problemWords("clangd", { kind: "not_this_server", first_line: "git version 2.49.0.windows.1" }),
  ).toBe("This isn't clangd: git version 2.49.0.windows.1");
  expect(problemWords("ty", { kind: "not_this_server", first_line: "" })).toBe(
    "This isn't ty: it printed nothing",
  );
  expect(problemWords("clangd", { kind: "did_not_start", cause: "Access is denied. (os error 5)" })).toBe(
    "clangd didn't start: Access is denied. (os error 5)",
  );
  expect(problemWords("clangd", { kind: "stopped", code: 1, stderr_tail: "" })).toBe(
    "clangd stopped (exit code 1)",
  );
  expect(problemWords("rust-analyzer", { kind: "stopped", code: null, stderr_tail: "" })).toBe(
    "rust-analyzer stopped",
  );
});

it("says why clangd can't answer, with what to do about it on Windows", () => {
  const builtIns = { kind: "built_in_headers_missing" } as const;
  expect(problemWords("clangd", builtIns, "Win32")).toBe(
    "clangd's built-in headers are missing. Unpack the whole archive, not just clangd.exe",
  );
  expect(problemWords("clangd", builtIns, "MacIntel")).toBe(
    "clangd's built-in headers are missing. Unpack the whole archive, not just clangd",
  );
  const system = { kind: "system_headers_missing", header: "stdlib.h" } as const;
  expect(problemWords("clangd", system, "Win32")).toBe(
    "clangd can't find system headers: stdlib.h. Install the MSVC build tools or the Windows SDK",
  );
  expect(problemWords("clangd", system, "MacIntel")).toBe("clangd can't find system headers: stdlib.h");
  expect(problemWords("clangd", { kind: "not_indexing" })).toBe(
    "clangd isn't indexing. Definitions in other files will land on header declarations",
  );
});

it("names each state of a server, and a problem in its words", () => {
  const states = (["not_set_up", "ready", "running", "idle"] as const).map((state) =>
    statusWords("ty", { state }),
  );
  expect(states).toEqual(["Not set up", "Ready", "Running", "Stopped (idle)"]);
  const problem = { kind: "did_not_start", cause: "not found" } as const;
  expect(statusWords("ty", { state: "problem", problem })).toBe("ty didn't start: not found");
});

it("shows the error output's tail only of a server that stopped and wrote some", () => {
  const stopped = (stderr_tail: string) =>
    stderrTail({ state: "problem", problem: { kind: "stopped", code: 3, stderr_tail } });

  expect(stopped("clangd: out of memory\r\n")).toBe("clangd: out of memory");
  expect(stopped("\n")).toBeUndefined();
  expect(stderrTail({ state: "problem", problem: { kind: "did_not_start", cause: "gone" } })).toBeUndefined();
  expect(stderrTail({ state: "running" })).toBeUndefined();
});

it("says which versions we tested, and the one found if it is another", () => {
  expect(versionWords({ version: null, tested: ["0.0.103"] })).toEqual({
    words: "Tested with 0.0.103",
    untested: false,
  });
  expect(versionWords({ version: { version: "0.0.110", tested: false }, tested: ["0.0.103"] })).toEqual({
    words: "Tested with 0.0.103; you have 0.0.110",
    untested: true,
  });
});
