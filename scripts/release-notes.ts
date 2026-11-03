// The rules a tag has to meet before the release workflow builds anything (ADR 0012).

/** The lines after the line `heading`, up to the next line that starts with `nextHeading`. */
function sectionOf(text: string, heading: string, nextHeading: string): string[] | undefined {
  const lines = text.split("\n");
  const start = lines.indexOf(heading);
  if (start === -1) {
    return undefined;
  }
  const after = lines.slice(start + 1);
  const next = after.findIndex((line) => line.startsWith(nextHeading));
  return next === -1 ? after : after.slice(0, next);
}

function workspaceVersion(cargoToml: string): string | undefined {
  return sectionOf(cargoToml, "[workspace.package]", "[")
    ?.map((line) => /^version = "(.+)"$/.exec(line)?.[1])
    .find((found) => found !== undefined);
}

/** The version `tag` releases. The installers take theirs from Cargo.toml, so the two have to agree. */
export function versionOfTag(tag: string, cargoToml: string): string {
  const version = workspaceVersion(cargoToml);
  if (version === undefined) {
    throw new Error("Cargo.toml has no version in [workspace.package].");
  }
  if (tag !== `v${version}`) {
    throw new Error(`The tag ${tag} doesn't match version ${version} in Cargo.toml.`);
  }
  return version;
}

/** The text of the changelog's section for `version`, without its heading. */
export function notesFor(changelog: string, version: string): string {
  const section = sectionOf(changelog, `## ${version}`, "## ");
  if (section === undefined) {
    throw new Error(`CHANGELOG.md has no section for ${version}.`);
  }
  const notes = section.join("\n").trim();
  if (notes === "") {
    throw new Error(`CHANGELOG.md's section for ${version} is empty.`);
  }
  return notes;
}
