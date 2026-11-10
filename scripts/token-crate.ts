// The rule behind `npm run token-deps`: the crate that holds the GitHub token may depend only on the crates
// on its list (ADR 0019). A new one needs a reviewed change to `ALLOWED`.

/** The direct dependencies `crates/token` may have: the HTTP client, its TLS errors and URLs. */
export const ALLOWED = ["ureq", "rustls", "url"];

/** A key at the start of a line, bare or quoted, before a `.` or an `=`. */
const KEY = /^(["']?)([A-Za-z0-9_-]+)\1\s*[.=]/;
/** A `package = "name"` that renames a dependency, inline or on a line of its own. */
const PACKAGE = /(?:^|[{,\s])package\s*=\s*["']([^"']+)["']/;

/** `line` without a comment after `#`. A dependency's name and version hold no `#`. */
function withoutComment(line: string): string {
  return line.replace(/#.*$/, "").trim();
}

/** The parts of a table header such as `[target.'cfg(windows)'.dependencies]`, quotes and all. */
function headerParts(line: string): string[] {
  const inside = line.replace(/^\[+/, "").replace(/\]+$/, "");
  return Array.from(inside.matchAll(/'[^']*'|"[^"]*"|[^.]+/g), (part) => part[0]);
}

/**
 * The packages a Cargo manifest names in its dependency tables, whatever the table or the target. A
 * dependency renamed with `package =` counts by the package it names, since that is what gets linked.
 */
export function dependencies(manifest: string): string[] {
  const found: string[] = [];
  let table: "list" | "one" | "other" = "other";
  for (const raw of manifest.split("\n")) {
    const line = withoutComment(raw);
    if (line.startsWith("[")) {
      const parts = headerParts(line).map((part) => part.replace(/^["']|["']$/g, ""));
      const at = parts.findIndex((part) => /^(dev-|build-)?dependencies$/.test(part));
      const named = at === -1 ? undefined : parts[at + 1];
      table = at === -1 ? "other" : named === undefined ? "list" : "one";
      if (named !== undefined) found.push(named);
    } else if (table === "list") {
      const key = KEY.exec(line);
      if (key !== null) found.push(PACKAGE.exec(line)?.[1] ?? String(key[2]));
    } else if (table === "one") {
      const renamed = PACKAGE.exec(line)?.[1];
      if (renamed !== undefined) found[found.length - 1] = renamed;
    }
  }
  return found;
}

/** What to say about each dependency of `manifest` that isn't on the list. */
export function problems(manifest: string): string[] {
  return dependencies(manifest)
    .filter((name) => !ALLOWED.includes(name))
    .map(
      (name) =>
        `crates/token/Cargo.toml depends on ${name}, which isn't on its list (${ALLOWED.join(", ")}). ` +
        "ADR 0019 keeps the crate that holds the token small, so an auditor can read all of it. " +
        "Move the code to the core, or add the crate to ALLOWED in scripts/token-crate.ts in a reviewed PR.",
    );
}
