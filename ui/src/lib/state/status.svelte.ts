// The app-wide status the core owns: asked for once, then kept current by its events (ADR 0008).
import { commands, events, type LastSync, type Status_Serialize as Status } from "$lib/data/bindings";

export type { LastSync, Status };

export class StatusState {
  status = $state<Status>();
  /** The open Repository's last sync. Nothing, until the core has a Repository open. */
  lastSync = $state<LastSync | null>(null);
  error = $state<string>();

  async load() {
    try {
      await events.statusChanged.listen(({ payload }) => {
        this.status = payload;
        this.error = undefined;
      });
      await events.lastSyncChanged.listen(({ payload }) => {
        this.lastSync = payload;
      });
      const current = await commands.status();
      const lastSync = await commands.lastSync();
      // An event that arrived while asking is newer than these answers.
      this.status ??= current;
      this.lastSync ??= lastSync;
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Asks the core to refresh the open Repository now. The answers come as events. */
  async refresh() {
    try {
      await commands.refresh();
    } catch (e) {
      this.error = String(e);
    }
  }
}
