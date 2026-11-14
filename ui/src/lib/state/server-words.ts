// What the window says of each language server (#683): its name and language, its state on its row in
// Settings › Advanced, and its last problem there and in the Definition peek. The core owns the problems
// (`ServerProblem`), and a new kind of one needs its words here.

import type { ServerName as Name, ServerProblem, ServerRow, ServerStatus } from "$lib/data/bindings";

/** A language server Tsugiki talks to. */
export type ServerName = Name;

/** How the window names each server, and the language it answers in. */
export const SERVERS: Readonly<Record<ServerName, { name: string; language: string }>> = {
  ty: { name: "ty", language: "Python" },
  "rust-analyzer": { name: "rust-analyzer", language: "Rust" },
  clangd: { name: "clangd", language: "C" },
};

/** The servers in the order their rows show in Settings › Advanced. */
export const SERVER_ORDER: readonly ServerName[] = ["ty", "clangd", "rust-analyzer"];

/** The line that says what went wrong with `server`, on the OS `platform` names. clangd's program is
 * `clangd.exe` on Windows, where the MSVC build tools or the Windows SDK hold the system's headers. */
export function problemWords(
  server: ServerName,
  problem: ServerProblem,
  platform: string = navigator.platform,
): string {
  const name = SERVERS[server].name;
  const windows = platform.startsWith("Win");
  switch (problem.kind) {
    case "built_in_headers_missing":
      return `${name}'s built-in headers are missing. Unpack the whole archive, not just ${windows ? `${name}.exe` : name}`;
    case "system_headers_missing": {
      const words = `${name} can't find system headers: ${problem.header}`;
      return windows ? `${words}. Install the MSVC build tools or the Windows SDK` : words;
    }
    case "not_indexing":
      return `${name} isn't indexing. Definitions in other files will land on header declarations`;
    case "not_this_server":
      return `This isn't ${name}: ${problem.first_line === "" ? "it printed nothing" : problem.first_line}`;
    case "did_not_start":
      return `${name} didn't start: ${problem.cause}`;
    case "stopped":
      return typeof problem.code === "number"
        ? `${name} stopped (exit code ${String(problem.code)})`
        : `${name} stopped`;
  }
}

/** What a server's row says of its state. */
export function statusWords(server: ServerName, status: ServerStatus): string {
  switch (status.state) {
    case "not_set_up":
      return "Not set up";
    case "ready":
      return "Ready";
    case "running":
      return "Running";
    case "idle":
      return "Stopped (idle)";
    case "problem":
      return problemWords(server, status.problem);
  }
}

/** The end of what a server that stopped on its own wrote to its error output, which its row shows under
 * its state, if it wrote anything. */
export function stderrTail(status: ServerStatus): string | undefined {
  if (status.state !== "problem" || status.problem.kind !== "stopped") return undefined;
  const tail = status.problem.stderr_tail.trimEnd();
  return tail === "" ? undefined : tail;
}

/** What a server's row says of its version: the versions we tested, and the one its check found, with
 * whether it is another, which Tsugiki still uses. */
export function versionWords(row: Pick<ServerRow, "version" | "tested">): {
  words: string;
  untested: boolean;
} {
  const tested = `Tested with ${row.tested.join(", ")}`;
  const found = row.version;
  if (found === undefined || found === null) return { words: tested, untested: false };
  if (found.tested) return { words: `Version ${found.version}, the one we tested`, untested: false };
  return { words: `${tested}; you have ${found.version}`, untested: true };
}
