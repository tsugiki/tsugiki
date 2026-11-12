// What the Detail pane shows, an Issue or a PR: opened once, asked for again each time the core says its
// Repository changed (ADR 0008), and read from GitHub about once a minute while it's on screen (ADR 0015).
// An Issue view stays hidden while a PR from its tree is on show, and asks nobody anything until it shows.
import { events, type ReadFailure } from "$lib/data/bindings";
import { hud } from "./hud.svelte";

export type { ReadFailure };

const MINUTE = 60_000;

/** What a command answers, as the generated bindings type it. */
type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

/** How the core opens, refreshes and reads one kind of item. */
export type DetailCommands<View> = {
  open: (repository: string, number: number) => Promise<Result<null, { message: string }>>;
  fetch: (repository: string, number: number) => Promise<Result<null, { message: string }>>;
  read: (repository: string, number: number) => Promise<Result<View | null, { message: string }>>;
};

export class DetailState<View extends { content: unknown }> {
  /** Undefined until the core answers, and null if Tsugiki doesn't hold this item. */
  view = $state<View | null>();
  error = $state<string>();
  /** The reason the last read from GitHub failed. Nothing, until one fails, and nothing again once a later
   * read saves an item Tsugiki didn't hold. A view that holds what the item says shows that instead. */
  failedRead = $state<ReadFailure>();
  readonly number: number;
  #repository: string;
  #commands: DetailCommands<View>;
  #lastQuestion = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;
  #reading: ReturnType<typeof setInterval> | undefined;
  #hidden = false;
  /** Whether the core announced a change to the Repository while this view was hidden. */
  #missedChange = false;
  /** Whether a minute's read from GitHub came round while this view was hidden. */
  #missedRead = false;

  constructor(repository: string, number: number, commands: DetailCommands<View>) {
    this.#repository = repository;
    this.number = number;
    this.#commands = commands;
  }

  /** Listens before it opens the item, so it hears the core saving what GitHub says, or why it couldn't. */
  async load() {
    const listening = await Promise.allSettled([
      // A change can touch a PR in an Issue's tree, or give the Issue a new one, so any change asks again.
      events.changed.listen(({ payload }) => {
        if (payload.repository !== this.#repository) return;
        // Only a read from GitHub changes an item Tsugiki doesn't hold, so the last failure no longer stands.
        // A held item's failure stays, as Mark Seen changes it too, and what the item says replaces it.
        if (this.view === null && payload.items.includes(this.number)) this.failedRead = undefined;
        if (this.#hidden) this.#missedChange = true;
        else void this.#ask();
      }),
      events.readFailed.listen(({ payload }) => {
        if (payload.repository === this.#repository && payload.number === this.number)
          this.failedRead = payload.why;
      }),
    ]);
    // A listener that did start stops with the view, even when the other one couldn't.
    const stops = listening.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
    const stopListening = () => {
      for (const stop of stops) stop();
    };
    const refused = listening.find((result) => result.status === "rejected");
    if (refused) {
      // With no listener the view would go stale, and the next good answer would clear this error.
      stopListening();
      this.error = String(refused.reason);
      return;
    }
    if (this.#stopped) {
      stopListening();
      return;
    }
    this.#stopListening = stopListening;
    this.#reading = setInterval(() => {
      if (document.hidden) return;
      if (this.#hidden) this.#missedRead = true;
      else void this.#fetch();
    }, MINUTE);
    await this.open();
  }

  /** Shows what the core holds until GitHub answers, and tells the core the user opened this item. The
   * view asks first, as opening a PR waits for git to say whether the mirror holds its head. Opening asks
   * the core and GitHub afresh, so a hidden view has nothing left to catch up on when it shows. */
  async open() {
    this.#missedChange = false;
    this.#missedRead = false;
    const asking = this.#ask();
    await this.#say(this.#commands.open(this.#repository, this.number));
    await asking;
  }

  /** Keeps what it holds, and stops asking GitHub and the core, until `show`. */
  hide() {
    this.#hidden = true;
  }

  /** Shows what it held, then asks the core again if the Repository changed meanwhile, and GitHub if a
   * minute's read came round. */
  show() {
    this.#hidden = false;
    if (this.#missedChange) void this.#ask();
    if (this.#missedRead) void this.#fetch();
    this.#missedChange = false;
    this.#missedRead = false;
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
    clearInterval(this.#reading);
  }

  #fetch = () => this.#say(this.#commands.fetch(this.#repository, this.number));

  async #say(asked: ReturnType<DetailCommands<View>["open"]>) {
    const answer = await asked.catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    if (answer.status === "error") this.error = answer.error.message;
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await hud
      .timed(() => this.#commands.read(this.#repository, this.number))
      .catch((e: unknown) => ({
        status: "error" as const,
        error: { message: String(e) },
      }));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.view = answer.data;
      this.error = undefined;
      // A view that has stopped or hidden is no longer on screen, though its last answer can still arrive.
      if (!this.#stopped && !this.#hidden)
        hud.issueShown(this.number, answer.data !== null && answer.data.content !== null);
    } else {
      this.error = answer.error.message;
    }
  }
}
