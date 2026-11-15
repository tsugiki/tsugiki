// The rules a version has to meet before the release workflow builds anything (ADR 0012), and the text of the
// release page it posts.

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

export function workspaceVersion(cargoToml: string): string {
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

/** The version `tag` releases. The downloads take theirs from Cargo.toml, so the two have to agree. */
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

/**
 * What each release page says after its notes: which file is for which system, and how to check one. It
 * names the files by extension, so it stays right when Tauri or `portable-zip.ts` changes a name. The command
 * is the README's, as a test checks. Each paragraph is one line, since GitHub breaks a release's lines where
 * its body does. The headings sit at the level of the changelog's Added and Fixed.
 */
const DOWNLOADS = [
  "### Download",
  "",
  "- Windows: the `.zip`",
  "- Apple silicon Macs: the `.dmg`",
  "",
  "### Checking a download",
  "",
  "These files aren't code-signed, so Windows and macOS warn you when you open one. Each file has an " +
    "attestation, which shows that Tsugiki's release workflow built it. Check it with the " +
    "[GitHub CLI](https://cli.github.com/):",
  "",
  "```",
  "gh attestation verify <file> --repo tsugiki/tsugiki --signer-workflow tsugiki/tsugiki/.github/workflows/release.yml",
  "```",
  "",
  "If this fails, the file isn't the one Tsugiki's workflow built, so don't run it, and open an issue.",
].join("\n");

/** A heading, or a fence's first or last line: a line no other line joins. */
const STANDS_ALONE = /^(#{1,6} |\s*```)/;

/** A line that starts a block of its own: a heading, a list item or a fence. */
const STARTS_BLOCK = /^(#{1,6} |\s*- |\s*```)/;

/**
 * `markdown` with each line it wraps joined onto the line above, since GitHub breaks a release's lines where
 * its body does. A list item's indented lines join the item, and the lines inside a fence stay as they are.
 */
function unwrapped(markdown: string): string {
  const lines: string[] = [];
  let fenced = false;
  for (const line of markdown.split("\n")) {
    const previous = lines.at(-1);
    const continues =
      !fenced &&
      line !== "" &&
      !STARTS_BLOCK.test(line) &&
      previous !== undefined &&
      previous !== "" &&
      !STANDS_ALONE.test(previous);
    if (continues) {
      lines[lines.length - 1] = `${previous} ${line.trimStart()}`;
    } else {
      lines.push(line);
    }
    if (/^\s*```/.test(line)) {
      fenced = !fenced;
    }
  }
  return lines.join("\n");
}

/** The text of a release page: the changelog's `notes`, then how to pick and check a download. */
export function releasePage(notes: string): string {
  return `${unwrapped(notes)}\n\n${DOWNLOADS}`;
}

/** The notes of the version Cargo.toml says, or of the work so far when nothing has been released. */
export function notesOfWorkspace(cargoToml: string, changelog: string): string {
  const version = workspaceVersion(cargoToml);
  return version === NEVER_RELEASED
    ? notesUnder(changelog, "WIP", "## WIP", (line) => line === "## WIP")
    : notesFor(changelog, version);
}
