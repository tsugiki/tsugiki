// Reads lcov reports and merges them, so code that runs on only one OS still counts as covered (ADR 0009).

type FileCoverage = { lines: Map<number, number>; functions: Map<string, number> };
type Coverage = Map<string, FileCoverage>;

const FUNCTION = /^(\d+)(?:,\d+)?,(.*)$/;
const FUNCTION_HITS = /^(\d+),(.*)$/;

const add = <K>(counts: Map<K, number>, key: K, count: number) =>
  counts.set(key, (counts.get(key) ?? 0) + count);

const fileIn = (coverage: Coverage, path: string) => {
  const known = coverage.get(path);
  if (known) return known;
  const file: FileCoverage = { lines: new Map(), functions: new Map() };
  coverage.set(path, file);
  return file;
};

// One function, whichever build it came from: a mangled name differs between OSes by its leading
// underscores, and between a crate's test build and the build others depend on by its crate hash.
// A hash's length moves the offsets that back-references like `B3_` hold, so those lose their offsets too.
// The name stays in the key, because two closures can start on one line.
const functionKey = (line: string, name: string) =>
  `${line} ${name
    .replace(/^_+/, "")
    .replace(/Cs[0-9a-zA-Z]+_/g, "")
    .replace(/B[0-9a-zA-Z]*_/g, "B_")}`;

const parseLcov = (text: string): Coverage => {
  const coverage: Coverage = new Map();
  for (const record of text.split("end_of_record")) {
    const fields = record
      .split(/\r?\n/)
      .filter((line) => line.includes(":"))
      .map((line) => {
        const colon = line.indexOf(":");
        return [line.slice(0, colon), line.slice(colon + 1)] as const;
      });
    const source = fields.find(([tag]) => tag === "SF");
    if (!source) continue;
    const file = fileIn(coverage, source[1]);
    const keyOf = new Map<string, string>();
    for (const [tag, value] of fields) {
      if (tag === "FN") {
        const [, line, name] = value.match(FUNCTION) as unknown as [string, string, string];
        keyOf.set(name, functionKey(line, name));
        add(file.functions, functionKey(line, name), 0);
      }
      if (tag === "FNDA") {
        const [, hits, name] = value.match(FUNCTION_HITS) as unknown as [string, string, string];
        const key = keyOf.get(name);
        if (key === undefined) throw new Error(`${source[1]}: hits for an unknown function, ${name}`);
        add(file.functions, key, Number(hits));
      }
      if (tag === "DA") {
        const [line, hits] = value.split(",");
        add(file.lines, Number(line), Number(hits));
      }
    }
  }
  return coverage;
};

const mergeCoverage = (reports: Coverage[]): Coverage => {
  const merged: Coverage = new Map();
  for (const report of reports) {
    for (const [path, { lines, functions }] of report) {
      const file = fileIn(merged, path);
      for (const [line, count] of lines) add(file.lines, line, count);
      for (const [key, count] of functions) add(file.functions, key, count);
    }
  }
  return merged;
};

const unreached = (counts: [number, number][]) =>
  [...new Set(counts.filter(([, count]) => count === 0).map(([line]) => line))]
    .sort((a, b) => a - b)
    .join(", ");

const findGaps = (coverage: Coverage): string[] =>
  [...coverage].flatMap(([path, { lines, functions }]) => {
    const missingLines = unreached([...lines]);
    const missingFunctions = unreached([...functions].map(([key, count]) => [parseInt(key), count]));
    const parts = [
      missingLines && `lines ${missingLines}`,
      missingFunctions && `functions at ${missingFunctions}`,
    ];
    const gaps = parts.filter(Boolean).join("; ");
    return gaps ? [`${path}: ${gaps}`] : [];
  });

/** Rewrites a report's absolute paths relative to `root`, so reports from different machines can be merged. */
export const relativeLcov = (text: string, root: string) => {
  const prefix = `${root.replaceAll("\\", "/")}/`;
  return text.replace(/^SF:(.*)$/gm, (_, path: string) => {
    const file = path.replaceAll("\\", "/");
    return `SF:${file.startsWith(prefix) ? file.slice(prefix.length) : path}`;
  });
};

/**
 * Merges each OS's report and says what no OS's tests reached, one line per file. Empty means the gate passes.
 * A report with no files is a problem in itself: merged in, it would go unnoticed.
 */
export const checkReports = (reports: { path: string; text: string }[]): string[] => {
  const parsed = reports.map(({ path, text }) => ({ path, coverage: parseLcov(text) }));
  const empty = parsed.filter(({ coverage }) => coverage.size === 0);
  if (empty.length > 0) return empty.map(({ path }) => `${path}: no coverage data`);
  return findGaps(mergeCoverage(parsed.map(({ coverage }) => coverage)));
};
