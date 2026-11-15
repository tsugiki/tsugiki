// The command palette's commands the user ran, most recently run first, which the palette lists first
// (#556). The core owns the list and keeps it in `machine.toml` beside the recent Repositories, so it
// survives a restart and stays on this machine. The window holds the core's last answer, with each run whose
// save hasn't answered yet in front of it.
import { commands } from "$lib/data/bindings";
import type { Answer } from "./answer";
import type { Action } from "./keymap.svelte";

/** What the history knows a row by where its title changes with what it does, as Hide Sidebar turns into
 * Show Sidebar: the action it runs, or one of the names here for a row that runs no action. */
export type PaletteId = Action | "sort_list" | "toggle_quiet";

/** A palette row as the history knows it: by its `id`, if it has one, or else by its title. */
export interface Remembered {
  title: string;
  id?: PaletteId;
}

export class RecentCommands {
  /** The ids in the palette's order: the runs not saved yet, then the core's list. */
  #ids = $state<string[]>([]);
  /** The core's list, as it last said. */
  #core: string[] = [];
  /** The runs whose save hasn't answered, most recent first. */
  #unsaved: string[] = [];
  /** Whether a save has answered, whose list is newer than the first read's. */
  #saved = false;
  #readProblem = $state<string>();
  #saveProblem = $state<string>();
  /** The last save, which the next one waits for, so the saves reach the file in the order the user ran
   * the commands. The first doesn't wait for the read. */
  #saving: Promise<void> = Promise.resolve();

  /** Why the last run couldn't be saved, or else why the history couldn't be read. */
  get problem(): string | undefined {
    return this.#saveProblem ?? this.#readProblem;
  }

  /** Reads the list the core keeps, unless a save has answered with a newer one already. */
  async load(): Promise<void> {
    try {
      const read = await commands.recentCommands();
      if (!this.#saved) this.#take(read);
    } catch (e) {
      this.#readProblem = `Tsugiki can't read the palette's recent commands: ${String(e)}`;
    }
  }

  /** Puts `row` first until the core answers, and asks the core to save it once the save before has
   * finished. */
  ran(row: Remembered): Promise<void> {
    const id = row.id ?? row.title;
    this.#unsaved = [id, ...this.#unsaved];
    this.#take(this.#core);
    this.#saving = this.#saving.then(() => this.#save(id));
    return this.#saving;
  }

  /** Never rejects, as a rejection would stop every save after it. A save that fails leaves the core's list
   * as it was, and the row goes back to its place there. */
  async #save(id: string) {
    let answer: Answer<string[]>;
    try {
      answer = await commands.rememberCommand(id);
    } catch (e) {
      answer = { status: "error", error: { message: String(e) } };
    }
    // The oldest run waiting, as the saves go in the order of the runs.
    this.#unsaved.splice(this.#unsaved.lastIndexOf(id), 1);
    if (answer.status === "ok") {
      this.#saved = true;
      this.#saveProblem = undefined;
      this.#take(answer.data);
    } else {
      this.#saveProblem = `Tsugiki can't save the palette's recent commands: ${answer.error.message}`;
      this.#take(this.#core);
    }
  }

  /** Takes `core` as the core's list, with the runs not saved yet in front. */
  #take(core: string[]) {
    this.#core = core;
    this.#ids = [...this.#unsaved, ...core].filter((id, at, all) => all.indexOf(id) === at);
  }

  /** `rows` in the palette's order: those run, most recently run first, then the others in their own
   * order. */
  order<Row extends Remembered>(rows: readonly Row[]): Row[] {
    const place = (row: Row) => {
      const at = this.#ids.indexOf(row.id ?? row.title);
      return at === -1 ? this.#ids.length : at;
    };
    return rows.toSorted((one, other) => place(one) - place(other));
  }
}
