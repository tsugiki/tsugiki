// The app-wide status the core owns: asked for once, then kept current by its events (ADR 0008).
import {
  commands,
  events,
  type Cloning,
  type HourPoints_Serialize as HourPoints,
  type InFlight_Serialize as InFlight,
  type LastSync,
  type MirrorProblem,
  type MirrorState_Serialize as MirrorState,
  type NewVersion,
  type PointsBack_Serialize as PointsBack,
  type RepositoryMirror_Serialize as RepositoryMirror,
  type Status_Serialize as Status,
  type SyncActivity,
} from "$lib/data/bindings";

export type {
  Cloning,
  HourPoints,
  InFlight,
  LastSync,
  MirrorProblem,
  MirrorState,
  NewVersion,
  PointsBack,
  RepositoryMirror,
  Status,
  SyncActivity,
};

/** Whether `status` says GitHub can't be reached, while Refresh asks it again too, as the core judges it:
 * a fetch then fails, so the window offers none (#981). The rate limit's gate does the same while it holds,
 * which `StatusState.pointsBack` says. */
export function isOffline(status: Status | undefined): boolean {
  switch (status?.state) {
    case "offline":
    case "offline_as":
    case "reconnecting":
    case "reconnecting_as":
      return true;
    case "online":
    case "connecting":
    case "connecting_as":
    case "problem":
    case undefined:
      return false;
  }
}

export class StatusState {
  status = $state<Status>();
  /** The open Repository's last sync. Nothing, until the core has a Repository open. */
  lastSync = $state<LastSync | null>(null);
  /** The queue of syncs' work. Nothing, while it's empty. */
  syncActivity = $state<SyncActivity | null>(null);
  /** An event has said, so an answer asked for before it is out of date. */
  #heardActivity = false;
  /**
   * The hour's GitHub points. Nothing, until GitHub's first answer of the run says, and from the hour's reset
   * until an answer of the next hour says.
   */
  hourPoints = $state<HourPoints | null>(null);
  #heardPoints = false;
  /** The time GitHub's points come back, while the rate limit's gate holds every request to GitHub. Nothing,
   * while it lets them through. */
  pointsBack = $state<PointsBack | null>(null);
  #heardBack = false;
  /** The request to GitHub in flight. Nothing, between requests. */
  inFlight = $state<InFlight | null>(null);
  #heardInFlight = false;
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
  /** A version newer than the running one. Nothing, until a check finds one, and the core never takes it
   * back. */
  newVersion = $state<NewVersion | null>(null);
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
      await events.hourPointsChanged.listen(({ payload }) => {
        this.hourPoints = payload;
        this.#heardPoints = true;
      });
      await events.pointsBackChanged.listen(({ payload }) => {
        this.pointsBack = payload;
        this.#heardBack = true;
      });
      await events.inFlightChanged.listen(({ payload }) => {
        this.inFlight = payload;
        this.#heardInFlight = true;
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
      await events.newVersionFound.listen(({ payload }) => {
        this.newVersion = payload;
      });
      const current = await commands.status();
      const lastSync = await commands.lastSync();
      const syncActivity = await commands.syncActivity();
      const hourPoints = await commands.hourPoints();
      const pointsBack = await commands.pointsBack();
      const inFlight = await commands.inFlight();
      const cloning = await commands.cloning();
      const mirrorProblem = await commands.mirrorProblem();
      const mirrorQuestion = await commands.mirrorQuestion();
      const mirrorStates = await commands.mirrorStates();
      const newVersion = await commands.newVersion();
      // An event that arrived while asking is newer than these answers.
      this.status ??= current;
      this.lastSync ??= lastSync;
      if (!this.#heardActivity) this.syncActivity = syncActivity;
      if (!this.#heardPoints) this.hourPoints = hourPoints;
      if (!this.#heardBack) this.pointsBack = pointsBack;
      if (!this.#heardInFlight) this.inFlight = inFlight;
      if (!this.#heardCloning) this.cloning = cloning;
      if (!this.#heardProblem) this.mirrorProblem = mirrorProblem;
      if (!this.#heardQuestion) this.mirrorQuestion = mirrorQuestion;
      if (!this.#heardStates) this.mirrorStates = mirrorStates;
      this.newVersion ??= newVersion;
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
