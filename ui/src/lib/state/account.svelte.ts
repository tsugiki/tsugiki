// The Settings dialog's Account page (docs/devel/token.md). The core says where the token comes from, and
// who is signed in comes with the app-wide status.
import { commands, type Login, type TokenSource } from "$lib/data/bindings";
import type { About } from "./about.svelte";
import type { Status } from "./status.svelte";

/** Who is signed in as the Account page says it: the login, if one is known, and a note when the login is
 * last time's or there is none. */
export type SignedIn = { login: Login; note?: string } | { login?: undefined; note: string };

/** Who the status says is signed in. A problem names nobody, and says what it is only when it is the
 * token's: a refused token or none found is the Account page's business, and a failed read of the disk
 * isn't. */
export function signedIn(status: Status | undefined): SignedIn {
  switch (status?.state) {
    case "online":
      return { login: status.login };
    case "connecting_as":
    case "reconnecting_as":
      return { login: status.last_login, note: "Signed in last time. GitHub hasn't answered yet." };
    case "offline_as":
      return { login: status.last_login, note: "Signed in last time. GitHub can't be reached now." };
    case "offline":
      return { note: "Not known: GitHub can't be reached." };
    case "problem":
      return {
        note:
          status.problem.kind === "unauthorized" || status.problem.kind === "no_token"
            ? status.problem.message
            : "Not known: GitHub hasn't said who is signed in.",
      };
    case "connecting":
    case "reconnecting":
    case undefined:
      return { note: "Not known yet: GitHub hasn't answered." };
  }
}

/** The token crate's README, which says what it does for readers who don't read Rust: the one in this
 * build's source, or on `main` until the core has said which build this is. */
export function tokenReadme(about: About | undefined): string {
  return `https://github.com/tsugiki/tsugiki/blob/${about?.source_ref ?? "main"}/crates/token/README.md`;
}

export class AccountState {
  source = $state<TokenSource>();
  error = $state<string>();

  async load() {
    try {
      this.source = await commands.tokenSource();
    } catch (e) {
      this.error = String(e);
    }
  }
}
