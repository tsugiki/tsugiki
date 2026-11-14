import { beforeEach, expect, it, vi } from "vitest";
import type { DefinitionCode, Hit } from "$lib/data/bindings";
import { CODE_CHUNK, CODE_SPANS_WAIT, HitCode } from "./hit-code.svelte";

const core = vi.hoisted(() => ({
  definitionCode: vi.fn<(code: DefinitionCode) => Promise<unknown>>(),
  definitionSpans: vi.fn<(code: DefinitionCode) => Promise<unknown>>(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { definitionCode: core.definitionCode, definitionSpans: core.definitionSpans },
}));

const ok = <T>(data: T) => ({ status: "ok" as const, data });

/** A file of `count` lines, each its own number. */
function answerFrom(count: number) {
  core.definitionCode.mockImplementation((code) => {
    const lines = [];
    for (let line = code.start; line < Math.min(code.end, count + 1); line++)
      lines.push(`line ${String(line)}`);
    return Promise.resolve(ok({ count, lines }));
  });
}

const atBase: Hit = {
  place: { kind: "repository", path: "Objects/unicodeobject.c", side: "base", commit: "abc" },
  line: 450,
  column: 0,
  text: "",
};

beforeEach(() => {
  core.definitionCode.mockReset();
  core.definitionSpans.mockReset();
  core.definitionSpans.mockResolvedValue(ok([]));
});

const keyword = { start: 0, end: 4, highlight: "keyword" as const };

/** Spans for each line `code` asks for: its first word a keyword. */
const keywords = (code: DefinitionCode) => ok(Array.from({ length: code.end - code.start }, () => [keyword]));

it("shows the lines with their spans when the spans come in time", async () => {
  answerFrom(1000);
  core.definitionSpans.mockImplementation((code) => Promise.resolve(keywords(code)));
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(447, 464);
  await vi.waitFor(() => {
    expect(code.line(450)).toBe("line 450");
  });

  expect(code.spans(450)).toEqual([keyword]);
  expect(code.spans(601)).toBeUndefined();
  const file = { kind: "repository", path: "Objects/unicodeobject.c", commit: "abc" };
  expect(core.definitionSpans).toHaveBeenCalledWith({
    repository: "python/cpython",
    file,
    start: 2 * CODE_CHUNK + 1,
    end: 3 * CODE_CHUNK + 1,
  });
});

it("shows the lines plain while the core parses the file, and paints them when the spans come", async () => {
  vi.useFakeTimers();
  answerFrom(1000);
  let parsed: (spans: unknown) => void = () => {};
  core.definitionSpans.mockReturnValue(new Promise((resolve) => (parsed = resolve)));
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(1, 18);
  await vi.advanceTimersByTimeAsync(CODE_SPANS_WAIT);
  const plain = [code.line(1), code.spans(1)];
  parsed(ok([[keyword]]));
  await vi.advanceTimersByTimeAsync(0);
  vi.useRealTimers();

  expect(plain).toEqual(["line 1", undefined]);
  expect(code.spans(1)).toEqual([keyword]);
});

it("leaves the lines plain when their spans fail", async () => {
  answerFrom(10);
  core.definitionSpans.mockResolvedValue({ status: "error", error: { message: "git failed" } });
  const failing = new HitCode("python/cpython", atBase, 0);
  core.definitionSpans.mockRejectedValueOnce(new Error("The window lost the core."));
  const lost = new HitCode("python/cpython", atBase, 1);

  lost.show(1, 18);
  failing.show(1, 18);
  await vi.waitFor(() => {
    expect([lost.line(1), failing.line(1)]).toEqual(["line 1", "line 1"]);
  });
  await new Promise((resolve) => setTimeout(resolve, 0));

  expect([lost.spans(1), failing.spans(1)]).toEqual([undefined, undefined]);
});

it("reads the chunks that hold the lines that show, at the hit's commit, once each", async () => {
  answerFrom(1000);
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(447, 464);
  code.show(447, 464);
  await vi.waitFor(() => {
    expect(code.count).toBe(1000);
  });

  const file = { kind: "repository", path: "Objects/unicodeobject.c", commit: "abc" };
  expect(core.definitionCode.mock.calls).toEqual([
    [{ repository: "python/cpython", file, start: 2 * CODE_CHUNK + 1, end: 3 * CODE_CHUNK + 1 }],
  ]);
  expect(code.line(450)).toBe("line 450");
  expect(code.line(401)).toBe("line 401");
  expect(code.line(601)).toBeUndefined();
});

it("reads a chunk across a boundary as two, and none past the file's end", async () => {
  answerFrom(250);
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(190, 210);
  await vi.waitFor(() => {
    expect(code.line(210)).toBe("line 210");
  });
  code.show(240, 900);

  expect(core.definitionCode.mock.calls.map(([asked]) => asked.start)).toEqual([1, 201]);
});

it("reads a file outside the Repository by its path on disk", async () => {
  answerFrom(3);
  const outside: Hit = {
    place: { kind: "outside", path: "C:/ty/builtins.pyi" },
    line: 2,
    column: 4,
    text: "",
  };
  const code = new HitCode("python/cpython", outside, 0);

  code.show(1, 18);
  await vi.waitFor(() => {
    expect(code.count).toBe(3);
  });

  expect(core.definitionCode).toHaveBeenCalledWith({
    repository: "python/cpython",
    file: { kind: "outside", path: "C:/ty/builtins.pyi" },
    start: 1,
    end: CODE_CHUNK + 1,
  });
});

it("says why when the window loses the core", async () => {
  core.definitionCode.mockRejectedValue(new Error("The window lost the core."));
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(1, 18);

  await vi.waitFor(() => {
    expect(code.problem).toBe("Error: The window lost the core.");
  });
});

it("keeps the lines it has when a later chunk fails, and asks for that chunk again", async () => {
  answerFrom(1000);
  const code = new HitCode("python/cpython", atBase, 0);
  code.show(1, 18);
  await vi.waitFor(() => {
    expect(code.line(1)).toBe("line 1");
  });
  core.definitionCode.mockResolvedValueOnce({ status: "error", error: { message: "git failed" } });

  code.show(401, 418);
  await vi.waitFor(() => {
    expect(core.definitionCode).toHaveBeenCalledTimes(2);
  });
  await Promise.resolve();
  code.show(401, 418);

  await vi.waitFor(() => {
    expect(code.line(401)).toBe("line 401");
  });
  expect(code.problem).toBeUndefined();
  expect(code.line(1)).toBe("line 1");
});

it("says why the file can't be read", async () => {
  core.definitionCode.mockResolvedValue({
    status: "error",
    error: { message: "No language server named x." },
  });
  const code = new HitCode("python/cpython", atBase, 0);

  code.show(1, 18);

  await vi.waitFor(() => {
    expect(code.problem).toBe("No language server named x.");
  });
});
