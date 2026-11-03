// The rules a version has to meet before the release workflow builds anything (ADR 0012).

/** The version Cargo.toml says until the first release, which no changelog section names. */
const NEVER_RELEASED = "0.0.0";

/** The lines after each line `isHeading` picks, up to the next line that starts with `nextHeading`. */
function sectionsOf(text: string, isHeading: (line: string) => boolean, nextHeading: string): string[][] {
  const lines = text.split("\n");
  return lines.flatMap((line, at) => {
    if (!isHeading(line)) {
      return [];
    }
    const after = lines.slice(at + 1);
    const next = after.findIndex((other) => other.startsWith(nextHeading));
    return [next === -1 ? after : after.slice(0, next)];
  });
}

function workspaceVersion(cargoToml: string): string {
  const version = sectionsOf(cargoToml, (line) => line === "[workspace.package]", "[")
    .flat()
    .map((line) => /^version = "(.+)"$/.exec(line)?.[1])
    .find((found) => found !== undefined);
  if (version === undefined) {
    throw new Error("Cargo.toml has no version in [workspace.package].");
  }
  return version;
}

/** The text of the one section `isHeading` picks, without its heading. `shown` is that heading in words. */
function notesUnder(
  changelog: string,
  name: string,
  shown: string,
  isHeading: (line: string) => boolean,
): string {
  const sections = sectionsOf(changelog, isHeading, "## ");
  const [section] = sections;
  if (section === undefined) {
    throw new Error(`CHANGELOG.md has no section "${shown}".`);
  }
  if (sections.length > 1) {
    throw new Error(`CHANGELOG.md has ${String(sections.length)} sections for ${name}.`);
  }
  const notes = section.join("\n").trim();
  if (notes === "") {
    throw new Error(`CHANGELOG.md's section for ${name} is empty.`);
  }
  return notes;
}

/** The version `tag` releases. The installers take theirs from Cargo.toml, so the two have to agree. */
export function versionOfTag(tag: string, cargoToml: string): string {
  const version = workspaceVersion(cargoToml);
  if (tag !== `v${version}`) {
    throw new Error(`The tag ${tag} doesn't match version ${version} in Cargo.toml.`);
  }
  return version;
}

/** The text of the changelog's section for `version`. */
export function notesFor(changelog: string, version: string): string {
  return notesUnder(
    changelog,
    version,
    `## Version ${version} (YYYY-MM-DD)`,
    (line) => /^## Version (\S+) \(\d{4}-\d{2}-\d{2}\)$/.exec(line)?.[1] === version,
  );
}

/** The notes of the version Cargo.toml says, or of the work so far when nothing has been released. */
export function notesOfWorkspace(cargoToml: string, changelog: string): string {
  const version = workspaceVersion(cargoToml);
  return version === NEVER_RELEASED
    ? notesUnder(changelog, "WIP", "## WIP", (line) => line === "## WIP")
    : notesFor(changelog, version);
}
