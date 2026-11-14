// A way for any part of the window to ask for the Settings dialog at a language server's row, as the
// Definition peek's link does (#683). The page opens the dialog there unless another dialog is open.

import type { ServerName } from "$lib/data/bindings";

class SettingsAsk {
  /** The server whose row in Settings › Advanced was asked for, until the page takes the ask. */
  server = $state<ServerName>();

  /** Asks for Settings › Advanced at `server`'s row. */
  open(server: ServerName) {
    this.server = server;
  }

  /** The server asked for, if one was, which no longer waits. */
  take(): ServerName | undefined {
    const server = this.server;
    this.server = undefined;
    return server;
  }
}

/** The one ask the window shares. */
export const settingsAsk = new SettingsAsk();
