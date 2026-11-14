// Each language server's row in Settings › Advanced (#683), as the core's servers' thread last wrote it:
// its state, its last problem and the version its check found. The core sends each change as an event.
// Browse… and Open log on a row ask the core too.

import { commands, events, type ServerRow as Row } from "$lib/data/bindings";
import { ask } from "./answer";
import { type ServerName, SERVERS } from "./server-words";

/** A server's row: its state or last problem, the version its check found, and the versions we tested. */
export type ServerRow = Row;

export class LanguageServerRows {
  rows = $state<ServerRow[]>([]);
  /** Why the core couldn't be asked for the rows. */
  problem = $state<string>();
  /** An event has said, so an answer asked for before it is out of date. */
  #heard = false;

  /** Listens for each change, then asks for the rows the core holds, once the listener is in place, so a
   * change the core sends in between isn't lost. Gives the way to stop listening. */
  watch(): () => void {
    const listening = events.languageServersChanged
      .listen(({ payload }) => {
        this.rows = payload;
        this.#heard = true;
      })
      .catch((e: unknown) => {
        this.problem = `Tsugiki can't hear how the language servers change: ${String(e)}`;
        return () => {};
      });
    void listening.then(() => this.#ask());
    return () => {
      void listening.then((stop) => {
        stop();
      });
    };
  }

  /** The row of `server`, once the core has said. */
  row(server: ServerName): ServerRow | undefined {
    return this.rows.find((row) => row.server === server);
  }

  async #ask() {
    try {
      const rows = await commands.languageServers();
      if (!this.#heard) this.rows = rows;
    } catch (e) {
      this.problem = `Tsugiki can't say how the language servers are: ${String(e)}`;
    }
  }
}

/** The path typed or pasted for a server's program, without the spaces around it or the one pair of quotes
 * Explorer's "Copy as path" puts around it. Nothing for a blank path, which clears the server's. */
export function typedPath(text: string): string | null {
  const trimmed = text.trim();
  const unquoted = /^"(.*)"$/su.exec(trimmed)?.[1] ?? trimmed;
  return unquoted === "" ? null : unquoted;
}

/** What Browse… found: the program the user picked, nothing if they cancelled, or why the picker didn't
 * open. */
export type Picked = { path: string | null } | { problem: string };

/** Opens the system's file picker for a server's program. */
export async function pickProgram(): Promise<Picked> {
  const answer = await ask(commands.pickProgram());
  if (answer.status === "error")
    return { problem: `Tsugiki can't open the file picker: ${answer.error.message}` };
  return { path: answer.data };
}

/** Opens the log `server` writes for `repository`, and says why if it couldn't. */
export async function openServerLog(repository: string, server: ServerName): Promise<string | undefined> {
  const answer = await ask(commands.openServerLog(repository, server));
  if (answer.status === "ok") return undefined;
  return `Tsugiki can't open ${SERVERS[server].name}'s log: ${answer.error.message}`;
}
