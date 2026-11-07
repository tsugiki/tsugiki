// What the Detail pane shows, an Issue or a PR: opened once, asked for again each time the core says its
// Repository changed (ADR 0008), and read from GitHub about once a minute while it's on screen (ADR 0015).
import { events } from "$lib/data/bindings";
import { hud } from "./hud.svelte";

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
  readonly number: number;
  #repository: string;
  #commands: DetailCommands<View>;
  #lastQuestion = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;
  #reading: ReturnType<typeof setInterval> | undefined;

  constructor(repository: string, number: number, commands: DetailCommands<View>) {
    this.#repository = repository;
    this.number = number;
    this.#commands = commands;
  }

  /** Listens before it opens the item, so it hears the core saving what GitHub says. */
  async load() {
    try {
      // A change can touch a PR in an Issue's tree, or give the Issue a new one, so any change asks again.
      const stopListening = await events.changed.listen(({ payload }) => {
        if (payload.repository === this.#repository) void this.#ask();
      });
      if (this.#stopped) {
        stopListening();
        return;
      }
      this.#stopListening = stopListening;
    } catch (e) {
      // With no listener the view would go stale, and the next good answer would clear this error.
      this.error = String(e);
      return;
    }
    this.#reading = setInterval(() => {
      if (!document.hidden) void this.#fetch();
    }, MINUTE);
    await this.open();
  }

  /** Tells the core the user opened this item, then shows what it holds until GitHub answers. */
  async open() {
    await this.#say(this.#commands.open(this.#repository, this.number));
    await this.#ask();
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
      // A view that has stopped is no longer on screen, though its last answer can still arrive.
      if (!this.#stopped) hud.issueShown(this.number, answer.data !== null && answer.data.content !== null);
    } else {
      this.error = answer.error.message;
    }
  }
}
