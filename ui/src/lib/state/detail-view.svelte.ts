// What the Detail pane shows, an Issue or a PR: opened once, and asked for again each time the core says its
// Repository changed (ADR 0008). The core reads it from GitHub again about once a minute while the user can
// see it (ADR 0015), so the view tells the core when it hides, shows again and closes.
// An Issue view stays hidden while a PR from its tree is on show, and asks the core nothing until it shows.
import { commands, events, type ReadFailure } from "$lib/data/bindings";
import { ask, problemIn } from "./answer";
import { hud } from "./hud.svelte";

export type { ReadFailure };

/** What a command answers, as the generated bindings type it. */
type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

/** A command that tells the core what the user did with an item. */
type ItemMove = (repository: string, number: number) => Promise<Result<null, { message: string }>>;

/** How the core opens and reads one kind of item. */
export type DetailCommands<View> = {
  open: ItemMove;
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
  #hidden = false;
  /** Whether the core announced a change to the Repository while this view was hidden. */
  #missedChange = false;
  /** Whether this view told the core the user opened its item. Until then it has nothing else to tell. */
  #opened = false;
  /** The last thing this view told the core, once the core has answered it. */
  #told: Promise<unknown> = Promise.resolve();

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
    await this.open();
  }

  /** Shows what the core holds until GitHub answers, and tells the core the user opened this item, which
   * reads it from GitHub now and about once a minute while the user can see it. The view asks first, as
   * opening a PR waits for git to say whether the mirror holds its head. Opening asks the core and GitHub
   * afresh, so a hidden view has nothing left to catch up on when it shows. */
  async open() {
    this.#missedChange = false;
    // The core counts the views of each item, which each open once, so opening again closes first.
    this.#tell(commands.closeItem);
    this.#opened = true;
    const asking = this.#ask();
    const opening = this.#after(() => this.#commands.open(this.#repository, this.number));
    // Opening shows the item to the core, so a view opened while hidden says so after.
    if (this.#hidden) this.#tell(commands.hideItem);
    const problem = await opening;
    if (problem !== undefined) this.error = problem;
    await asking;
  }

  /** Keeps what it holds, and stops asking the core until `show`, which stops reading it from GitHub. */
  hide() {
    this.#hidden = true;
    this.#tell(commands.hideItem);
  }

  /** Shows what it held, then asks the core again if the Repository changed meanwhile. The core reads the
   * item from GitHub at once if its minute came round meanwhile. */
  show() {
    this.#hidden = false;
    this.#tell(commands.showItem);
    if (this.#missedChange) void this.#ask();
    this.#missedChange = false;
  }

  /** Stops listening, and tells the core the user left this item. */
  stop() {
    this.#tell(commands.closeItem);
    this.#stopped = true;
    this.#stopListening?.();
  }

  /** Hears each answer the core gives, once `view` holds it, for a view to pass on what it reads. */
  protected heard?(view: View | null): void;

  /** Tells the core the user did `move` with this item, once it has opened it. A refusal shows nothing: the
   * user asked for none of this, and the view still holds what it read. */
  #tell(move: ItemMove) {
    if (this.#opened) void this.#after(() => move(this.#repository, this.number));
  }

  /** Runs `telling` once the core has answered what this view told it before, as opening a PR answers
   * only once git has spoken, and a close sent meanwhile could reach the core first. Answers with the
   * problem the core ran into, if any. */
  #after(telling: () => ReturnType<ItemMove>): Promise<string | undefined> {
    const told = this.#told.then(() => problemIn(telling));
    this.#told = told;
    return told;
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await ask(hud.timed(() => this.#commands.read(this.#repository, this.number)));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.view = answer.data;
      this.heard?.(answer.data);
      this.error = undefined;
      // A view that has stopped or hidden is no longer on screen, though its last answer can still arrive.
      if (!this.#stopped && !this.#hidden)
        hud.issueShown(this.number, answer.data !== null && answer.data.content !== null);
    } else {
      this.error = answer.error.message;
    }
  }
}
