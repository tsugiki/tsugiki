// The Settings dialog's Account page (docs/devel/token.md). The core says where the token comes from, and
// who is signed in comes with the app-wide status.
import { commands, type Login, type TokenSource } from "$lib/data/bindings";
import type { About } from "./about.svelte";
import type { Status } from "./status.svelte";

/** Who is signed in as the Account page says it: the login, if one is known, and a note when the login is
 * last time's or there is none. */
export type SignedIn = { login: Login; note?: string } | { login?: undefined; note: string };

/** Who the status's account says is signed in. A refused token or none found names nobody, and says what
 * happened, as it is the Account page's business. A problem beside the account, such as a failed read of
 * the disk, isn't, so it changes nothing here. */
export function signedIn(status: Status | undefined): SignedIn {
  const account = status?.account;
  switch (account?.state) {
    case "online":
      return { login: account.login };
    case "connecting_as":
    case "reconnecting_as":
      return { login: account.last_login, note: "Signed in last time. GitHub hasn't answered yet." };
    case "offline_as":
      return { login: account.last_login, note: "Signed in last time. GitHub can't be reached now." };
    case "offline":
      return { note: "Not known: GitHub can't be reached." };
    case "unconfirmed_as":
      return { login: account.last_login, note: "Signed in last time. GitHub's last answer didn't say who is now." };
    case "unconfirmed":
      return { note: "Not known: GitHub's last answer didn't say." };
    case "stopped":
      return { note: "Not known: Tsugiki stopped asking GitHub. The status bar says why." };
    case "no_token":
    case "refused":
      return { note: account.message };
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
