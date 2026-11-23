// A way for any part of the window to ask for the Settings dialog at a place: a language server's row, as
// the Definition peek's link does (#683), or a Repository's page, as the link in a Read-only Repository's
// line does (#915). The page opens the dialog there unless another dialog is open.

import type { ServerName } from "$lib/data/bindings";

/** A place the Settings dialog opens at: a language server's row in Settings › Advanced, or a Repository's
 * page. */
export type SettingsAt = { server: ServerName } | { repository: string };

class SettingsAsk {
  /** The place asked for, until the page takes the ask. */
  asked = $state<SettingsAt>();

  /** Asks for Settings › Advanced at `server`'s row. */
  open(server: ServerName) {
    this.asked = { server };
  }

  /** Asks for the page of `repository` in Settings. */
  openRepository(repository: string) {
    this.asked = { repository };
  }

  /** The place asked for, if one was, which no longer waits. */
  take(): SettingsAt | undefined {
    const asked = this.asked;
    this.asked = undefined;
    return asked;
  }
}

/** The one ask the window shares. */
export const settingsAsk = new SettingsAsk();
