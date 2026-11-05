// The app-wide status the core owns: asked for once, then kept current by its events (ADR 0008).
import {
  commands,
  events,
  type LastSync,
  type Status_Serialize as Status,
  type SyncActivity,
} from "$lib/data/bindings";

export type { LastSync, Status, SyncActivity };

export class StatusState {
  status = $state<Status>();
  /** The open Repository's last sync. Nothing, until the core has a Repository open. */
  lastSync = $state<LastSync | null>(null);
  /** The queue of syncs' work. Nothing, while it's empty. */
  syncActivity = $state<SyncActivity | null>(null);
  /** An event has said, so an answer asked for before it is out of date. */
  #heardActivity = false;
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
      await events.syncActivityChanged.listen(({ payload }) => {
        this.syncActivity = payload;
        this.#heardActivity = true;
      });
      const current = await commands.status();
      const lastSync = await commands.lastSync();
      const syncActivity = await commands.syncActivity();
      // An event that arrived while asking is newer than these answers.
      this.status ??= current;
      this.lastSync ??= lastSync;
      if (!this.#heardActivity) this.syncActivity = syncActivity;
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
