// Counts the rows each file shows.
import { readFile } from "node:fs/promises";

export async function count(path, { skip = 0 } = {}) {
  const text = await readFile(path, "utf8");
  const lines = text.split(/\r?\n/).slice(skip);
  if (lines.length === 0) return null;
  return lines.filter((line) => line.startsWith("+")).length;
}

class Counter extends Map {
  static MAX = 1_000;
}

const view = <Row count={count("a.diff")} label="added" />;
