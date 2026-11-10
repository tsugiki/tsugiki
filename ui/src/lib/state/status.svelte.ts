// The app-wide status the core owns: asked for once, then kept current by its events (ADR 0008).
import {
  commands,
  events,
  type Cloning,
  type LastSync,
  type MirrorProblem,
  type MirrorState_Serialize as MirrorState,
  type RepositoryMirror_Serialize as RepositoryMirror,
  type Status_Serialize as Status,
  type SyncActivity,
} from "$lib/data/bindings";

export type { Cloning, LastSync, MirrorProblem, MirrorState, RepositoryMirror, Status, SyncActivity };

export class StatusState {
  status = $state<Status>();
  /** The open Repository's last sync. Nothing, until the core has a Repository open. */
  lastSync = $state<LastSync | null>(null);
  /** The queue of syncs' work. Nothing, while it's empty. */
  syncActivity = $state<SyncActivity | null>(null);
  /** An event has said, so an answer asked for before it is out of date. */
  #heardActivity = false;
  /** A mirror's clone under way. Nothing, between clones. */
  cloning = $state<Cloning | null>(null);
  #heardCloning = false;
  /** The reason no mirror can be made. Nothing, while one can. */
  mirrorProblem = $state<MirrorProblem | null>(null);
  #heardProblem = false;
  /** The first Repository whose mirror waits to hear where it gets its objects. Nothing, while none waits. */
  mirrorQuestion = $state<string | null>(null);
  #heardQuestion = false;
  /** The state of each Repository's mirror the core has heard of. */
  mirrorStates = $state<RepositoryMirror[]>([]);
  #heardStates = false;
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
      await events.cloningChanged.listen(({ payload }) => {
        this.cloning = payload;
        this.#heardCloning = true;
      });
      await events.mirrorProblemChanged.listen(({ payload }) => {
        this.mirrorProblem = payload;
        this.#heardProblem = true;
      });
      await events.mirrorQuestionChanged.listen(({ payload }) => {
        this.mirrorQuestion = payload;
        this.#heardQuestion = true;
      });
      await events.mirrorStatesChanged.listen(({ payload }) => {
        this.mirrorStates = payload;
        this.#heardStates = true;
      });
      const current = await commands.status();
      const lastSync = await commands.lastSync();
      const syncActivity = await commands.syncActivity();
      const cloning = await commands.cloning();
      const mirrorProblem = await commands.mirrorProblem();
      const mirrorQuestion = await commands.mirrorQuestion();
      const mirrorStates = await commands.mirrorStates();
      // An event that arrived while asking is newer than these answers.
      this.status ??= current;
      this.lastSync ??= lastSync;
      if (!this.#heardActivity) this.syncActivity = syncActivity;
      if (!this.#heardCloning) this.cloning = cloning;
      if (!this.#heardProblem) this.mirrorProblem = mirrorProblem;
      if (!this.#heardQuestion) this.mirrorQuestion = mirrorQuestion;
      if (!this.#heardStates) this.mirrorStates = mirrorStates;
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

  /** Asks the core to look at git and the disk again. The answer comes as an event. */
  async checkMirrorsAgain() {
    try {
      await commands.checkMirrorsAgain();
    } catch (e) {
      this.error = String(e);
    }
  }
}
