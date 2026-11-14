// The Settings dialog's Account page (docs/devel/token.md). The core says where the token comes from, and
// who is signed in comes with the app-wide status.
import { commands, type Login, type TokenSource } from "$lib/data/bindings";
import type { Status } from "./status.svelte";

export type { TokenSource };

/** Who is signed in as the Account page says it: the login, if one is known, and a note when the login is
 * last time's or there is none. */
export type SignedIn = { login: Login; note?: string } | { login?: undefined; note: string };

/** Who the status says is signed in. */
export function signedIn(status: Status | undefined): SignedIn {
  switch (status?.state) {
    case "online":
      return { login: status.login };
    case "connecting_as":
      return { login: status.last_login, note: "Signed in last time. GitHub hasn't answered yet." };
    case "offline_as":
      return { login: status.last_login, note: "Signed in last time. GitHub can't be reached now." };
    case "offline":
      return { note: "Not known: GitHub can't be reached." };
    case "problem":
      return { note: status.problem.message };
    case "connecting":
    case undefined:
      return { note: "Not known yet: GitHub hasn't answered." };
  }
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
