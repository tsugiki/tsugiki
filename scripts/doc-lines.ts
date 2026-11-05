// The lines of a file that hold prose we wrote: all of a Markdown file, and the comments and docstrings of a
// source file. `just doc-width` measures them, and `just prose-check` reads them.

/** The widest a line of prose can run (AGENTS.md). */
export const WIDEST = 110;

const SLASH_COMMENTS = new Set(["rs", "ts", "svelte"]);
const HASH_COMMENTS = new Set(["yml", "yaml", "toml"]);
const GENERATED = new Set(["ui/src/lib/data/bindings.ts"]);

/** A line of prose: where it is, the line as the file has it, and its words without a comment's marks. */
export interface DocLine {
  at: number;
  raw: string;
  text: string;
}

/** A line past `WIDEST`. */
export interface WideLine {
  at: number;
  width: number;
}

function extensionOf(path: string): string {
  return path.slice(path.lastIndexOf(".") + 1);
}

function usesHashComments(path: string): boolean {
  return HASH_COMMENTS.has(extensionOf(path)) || path.endsWith("justfile");
}

export function isDocFile(path: string): boolean {
  const known = extensionOf(path) === "md" || SLASH_COMMENTS.has(extensionOf(path)) || usesHashComments(path);
  return known && !GENERATED.has(path);
}

/** Each line's comment text, for a file with `//`, `/*` and `<!--` comments. Code gets `undefined`. */
function slashComments(lines: string[]): (string | undefined)[] {
  let closing: string | undefined;
  return lines.map((line) => {
    let body = line.trim();
    if (closing === undefined) {
      if (body.startsWith("//")) {
        return body.replace(/^\/\/[/!]?/, "").trim();
      }
      const opening = /^(?:\/\*\*?|<!--)/.exec(body)?.[0];
      if (opening === undefined) {
        return undefined;
      }
      closing = opening === "<!--" ? "-->" : "*/";
      body = body.slice(opening.length);
    }
    const end = body.indexOf(closing);
    if (end !== -1) {
      body = body.slice(0, end);
      closing = undefined;
    }
    return body.replace(/^\s*\*/, "").trim();
  });
}

function hashComments(lines: string[]): (string | undefined)[] {
  return lines.map((line) => /^\s*#(.*)$/.exec(line)?.[1]?.trim());
}

function textsOf(path: string, lines: string[]): (string | undefined)[] {
  if (SLASH_COMMENTS.has(extensionOf(path))) {
    return slashComments(lines);
  }
  return usesHashComments(path) ? hashComments(lines) : lines.map((line) => line.trim());
}

export function docLines(path: string, source: string): DocLine[] {
  const lines = source.split(/\r?\n/);
  const texts = textsOf(path, lines);
  let inFence = false;
  return lines.flatMap((raw, index) => {
    const text = texts[index];
    if (text === undefined || text === "" || text.startsWith("|")) {
      return [];
    }
    if (/^(?:```|~~~)/.test(text)) {
      inFence = !inFence;
      return [];
    }
    return inFence ? [] : [{ at: index + 1, raw, text }];
  });
}

/** True when wrapping could shorten this line. A heading can't wrap, and neither can one long word. */
function canWrap(text: string): boolean {
  return !/^#{1,6}\s/.test(text) && text.replace(/^(?:[-*+]|\d+\.)\s+/, "").includes(" ");
}

export function wideLines(path: string, source: string): WideLine[] {
  return docLines(path, source)
    .map(({ at, raw, text }) => ({ at, text, width: [...raw].length }))
    .filter(({ text, width }) => width > WIDEST && canWrap(text))
    .map(({ at, width }) => ({ at, width }));
}
