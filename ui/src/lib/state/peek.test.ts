import { expect, it } from "vitest";
import type { LaidThread, ThreadAnchor } from "./diff-layout";
import { PEEK_ID, withPeek } from "./peek";

const laid = <At extends ThreadAnchor>(id: string, at: At) => ({ id, at, places: 3 });
const ids = (threads: LaidThread[]) => threads.map((thread) => thread.id);

const threads = [
  laid("row 2", { kind: "row", row: 2 }),
  laid("file 0 line 4", { kind: "line", file: 0, line: 4 }),
  laid("row 5", { kind: "row", row: 5 }),
  laid("file 1 line 9", { kind: "line", file: 1, line: 9 }),
  laid("end", { kind: "end" }),
];

it("puts the peek under a row after the threads above it and before those under it or below", () => {
  const under = (row: number) => ids(withPeek(threads, laid(PEEK_ID, { kind: "row", row })));

  expect(under(1).indexOf(PEEK_ID)).toBe(0);
  expect(under(2).indexOf(PEEK_ID)).toBe(0);
  expect(under(3).indexOf(PEEK_ID)).toBe(2);
  expect(under(9).indexOf(PEEK_ID)).toBe(5);
});

it("puts the peek under a line among the threads under lines of its own file", () => {
  const under = (file: number, line: number) =>
    ids(withPeek(threads, laid(PEEK_ID, { kind: "line", file, line })));

  expect(under(0, 4).indexOf(PEEK_ID)).toBe(1);
  expect(under(0, 7).indexOf(PEEK_ID)).toBe(5);
  expect(under(1, 2).indexOf(PEEK_ID)).toBe(3);
});

it("leaves the threads in their order", () => {
  const placed = withPeek(threads, laid(PEEK_ID, { kind: "row", row: 3 }));

  expect(ids(placed).filter((id) => id !== PEEK_ID)).toEqual(ids(threads));
});
