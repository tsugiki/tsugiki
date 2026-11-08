// Finds what the Rust coverage gate fell short on. cargo-llvm-cov can fail with no line to show: a closure or
// a `?` that never ran can sit on a line that did. `rust-coverage.ts` prints these when the gate fails. It
// also asks here whether an old build sits beside the new one, which adds false gaps.

/** A file short of 100%, with its counts as covered/found. */
export interface FileGap {
  path: string;
  lines: string;
  functions: string;
}

/** Source that never ran. `whole` means the line, and otherwise part of a line that did run. */
export interface UnrunRegion {
  at: number;
  source: string;
  whole: boolean;
}

/** Each file in an lcov report that's short of its lines or functions. */
export function filesWithGaps(lcov: string): FileGap[] {
  return lcov.split("end_of_record").flatMap((record) => {
    const count = (name: string) => new RegExp(`^${name}:(.*)$`, "m").exec(record)?.[1]?.trim();
    const [path, found, hit, functionsFound, functionsHit] = ["SF", "LF", "LH", "FNF", "FNH"].map(count);
    if (path === undefined || (found === hit && functionsFound === functionsHit)) {
      return [];
    }
    return [{ path, lines: `${hit}/${found}`, functions: `${functionsHit}/${functionsFound}` }];
  });
}

/** True when a line of markers points at an unrun region other than a `?`. */
function marksARegion(markers: string, line: string): boolean {
  return [...markers.matchAll(/\^0(?!\d)/g)].some(({ index }) => line[index] !== "?");
}

/**
 * The source in `path` that never ran, from the text of `cargo llvm-cov report --text`. It skips a `?` on a
 * line that ran: the gate counts lines and functions, so an unrun error path there can't fail it.
 */
export function unrunRegions(report: string, path: string): UnrunRegion[] {
  const lines = report.split(/\r?\n/);
  const start = lines.indexOf(`${path}:`);
  if (start === -1) {
    return [];
  }
  const regions: UnrunRegion[] = [];
  let above: { at: number; source: string } | undefined;
  let raw = "";
  for (const line of lines.slice(start + 1)) {
    const source = /^\s*(\d+)\|\s*([^|]*)\|/.exec(line);
    if (source !== null) {
      raw = line;
      above = { at: Number(source[1]), source: line.slice(source[0].length).trim() };
      if (source[2] === "0") {
        regions.push({ ...above, whole: true });
      }
    } else if (/^\S.*:$/.test(line)) {
      break;
    } else if (above !== undefined && marksARegion(line, raw)) {
      regions.push({ ...above, whole: false });
    }
  }
  return regions;
}

/**
 * Says whether `deps` holds two builds of one test target, which Cargo names `<target>-<hash>`, with `.exe`
 * on Windows. One configuration gives each target one hash, so a second comes from another toolchain,
 * `Cargo.lock` or set of flags. cargo-llvm-cov reads every build there, and code removed since the old one
 * shows up as never run.
 */
export function hasStaleBuilds(names: string[]): boolean {
  const targets = new Set<string>();
  for (const name of names) {
    const target = /^(.+)-[0-9a-f]{16}(?:\.exe)?$/.exec(name)?.[1];
    if (target === undefined) continue;
    if (targets.has(target)) return true;
    targets.add(target);
  }
  return false;
}
