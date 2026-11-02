// The app-wide status the core owns: asked for once, then kept current by its events (ADR 0008).
import { commands, events, type Status_Serialize as Status } from "$lib/data/bindings";

export type { Status };

export class StatusState {
  status = $state<Status>();
  error = $state<string>();

  async load() {
    try {
      await events.statusChanged.listen(({ payload }) => {
        this.status = payload;
        this.error = undefined;
      });
      const current = await commands.status();
      // An event that arrived while asking is newer than this answer.
      this.status ??= current;
    } catch (e) {
      this.error = String(e);
    }
  }
}
