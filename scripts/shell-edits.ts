// The PreToolUse hook in .claude/settings.json refuses a shell command that rewrites a file of the repo in
// place, with sed, perl, a Python or Node one-off, PowerShell's Set-Content, tee or a redirect. An agent
// that edits that way skips the Edit tool's exact match, so a pattern that misses changes nothing or breaks
// the file, and the next test run is the first to say so. Files under .temp stay open to the shell, and so
// do cp, mv and git mv, which move files whole.
//
// It nudges, and doesn't guard: a command that names a file from inside `ui`, or splits a sed script at a
// `|`, gets through. It errs that way so that a command that only reads the repo still runs.

const SOURCE_PATH =
  String.raw`((crates|ui|scripts|docs|\.github)[/\\]|(AGENTS|CHANGELOG|CONTEXT|CONTRIBUTING|README)\.md\b|` +
  String.raw`(justfile|Cargo\.toml|package\.json)\b)`;

/** A path to the repo's code or docs, from the repo's root or through the repo's own folder name. */
const SOURCE = new RegExp(String.raw`(^|[\s"'=(]|gh-rev[/\\])(\./)?` + SOURCE_PATH);

/** The same through the repo's folder name only, for a command that moves to a folder outside the repo. */
const ROOTED = new RegExp(String.raw`gh-rev[/\\]` + SOURCE_PATH);

/** A `cd` to a folder outside the repo, after which a relative path means a file there. */
const CD_ELSEWHERE = /(?:^|[;&|]\s*)cd\s+["']?(?![^\s"';&|]*gh-rev)/;

/** A writer that names its files in the same segment of the command, with what to call it. */
const IN_PLACE: [RegExp, string][] = [
  [/\bsed\s(.*\s)?(-[A-Za-z]*i|--in-place)\b/, "sed -i"],
  [/\bperl\s(.*\s)?-(?![Mm])\w*i/, "perl -i"],
  [/\b(Set-Content|Add-Content|Out-File)\b/, "PowerShell's Set-Content"],
  [/\btee\s/, "tee"],
];

/** A one-off script, often in a heredoc, that writes a file it names somewhere in its text. */
const SCRIPT: [RegExp, string][] = [
  [/\bwrite_text\(|\bopen\([^\n]*?["'][wax]b?\+?["']/, "a Python write"],
  [/\bwriteFileSync\(/, "a Node write"],
];

/** A redirect and its target. A digit, `&`, `-` or `=` before the `>` makes it `2>&1`, `->` or `=>`. */
const REDIRECT = /(?:^|[^<>&\d=-])>>?\s*["']?([^\s"'|;&<>]+)/g;

/** `command` without the bodies of its heredocs, which hold text for the command to read. */
function withoutHeredocs(command: string): string {
  const out: string[] = [];
  let end: string | undefined;
  for (const line of command.split("\n")) {
    if (end !== undefined) {
      end = line.trim() === end ? undefined : end;
      continue;
    }
    out.push(line);
    end = /<<-?\s*["']?(\w+)["']?/.exec(line)?.[1];
  }
  return out.join("\n");
}

function writer(command: string): string | undefined {
  const source = CD_ELSEWHERE.test(command) ? ROOTED : SOURCE;
  const script = SCRIPT.find(([pattern]) => pattern.test(command));
  if (script !== undefined && source.test(command)) {
    return script[1];
  }
  const lines = withoutHeredocs(command);
  for (const segment of lines.split(/\n|;|&&|\|\|?/)) {
    const found = IN_PLACE.find(([pattern]) => pattern.test(segment));
    if (found !== undefined && source.test(segment)) {
      return found[1];
    }
  }
  const targets = [...lines.matchAll(REDIRECT)].map(([, target]) => String(target));
  return targets.some((target) => source.test(target)) ? "a redirect" : undefined;
}

/** The hook's reason to refuse `command`, or `undefined` to let it run. */
export function shellEdit(command: string): string | undefined {
  const how = writer(command);
  if (how === undefined) {
    return undefined;
  }
  return (
    `This command looks like it rewrites a file of the repo with ${how}. ` +
    "Edit code and docs with the Edit or Write tool, which fails loudly when its text doesn't match. " +
    "Scratch files under .temp are fine to write from the shell."
  );
}
