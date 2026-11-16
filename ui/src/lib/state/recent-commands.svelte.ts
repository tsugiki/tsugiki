// The command palette's commands the user ran, most recently run first, which the palette lists first
// (#556). The core owns the list and keeps it in `machine.toml` beside the recent Repositories, so it
// survives a restart and stays on this machine. The window holds the core's last answer, with each run whose
// save hasn't answered yet in front of it.
import { commands } from "$lib/data/bindings";
import type { Answer } from "./answer";
import type { Action } from "./keymap.svelte";

/** What the history knows a row by where its title changes with what it does, as Hide Sidebar turns into
 * Show Sidebar: the action it runs, or one of the names here for a row that runs no action. */
export type PaletteId = Action | "sort_list";

/** A palette row as the history knows it: by its `id`, if it has one, or else by its title. */
export interface Remembered {
  title: string;
  id?: PaletteId;
  /** Keeps the row out of the history, as a switch to a Repository: those come in the order the
   * Repositories were opened, which the core keeps already. */
  history?: false;
}

export class RecentCommands {
  /** The core's list, as it last said. */
  #core = $state<string[]>([]);
  /** The runs whose save hasn't answered, most recent first. */
  #unsaved = $state<string[]>([]);
  /** Each id's place in the palette's order: the runs not saved yet, then the core's list. It is built anew
   * whenever either changes, so it needn't be reactive itself. */
  readonly #places = $derived(
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    new Map<string, number>(
      [...this.#unsaved, ...this.#core]
        .filter((id, at, all) => all.indexOf(id) === at)
        .map((id, at) => [id, at]),
    ),
  );
  /** How many saves have answered with the core's list. A read asked before one of them answered is older
   * than its answer. */
  #saves = 0;
  #readProblem = $state<string>();
  #saveProblem = $state<string>();
  /** The last save, which the next one waits for, so the saves reach the file in the order the user ran
   * the commands. The first doesn't wait for a read. */
  #saving: Promise<void> = Promise.resolve();

  /** Why the last run couldn't be saved, or else why the history couldn't be read. */
  get problem(): string | undefined {
    return this.#saveProblem ?? this.#readProblem;
  }

  /** Reads the list the core keeps: at start-up, and once the Settings dialog has read the files again, as
   * the user may have edited it by hand. A save that answers meanwhile has the newer list, and the read
   * goes, with what it would have said.
   *
   * A read taken forgets why the last run couldn't be saved too. That problem lay in `machine.toml`, and the
   * dialog, which read the file just now, names any problem the file still has in the status bar. The next
   * run says again if it still can't save. */
  async load(): Promise<void> {
    const saves = this.#saves;
    try {
      const read = await commands.recentCommands();
      if (this.#saves !== saves) return;
      this.#core = read;
      this.#readProblem = undefined;
      this.#saveProblem = undefined;
    } catch (e) {
      this.#readProblem = `Tsugiki can't read the palette's recent commands: ${String(e)}`;
    }
  }

  /** Puts `row` first until the core answers, and asks the core to save it once the save before has
   * finished. A row the history leaves out runs with nothing saved. */
  ran(row: Remembered): Promise<void> {
    if (row.history === false) return this.#saving;
    const id = row.id ?? row.title;
    this.#unsaved = [id, ...this.#unsaved];
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
    this.#unsaved = this.#unsaved.toSpliced(this.#unsaved.lastIndexOf(id), 1);
    if (answer.status === "ok") {
      this.#saves += 1;
      this.#core = answer.data;
      this.#saveProblem = undefined;
      // The core's answer is the whole list, so whatever kept the read from it has passed.
      this.#readProblem = undefined;
    } else {
      this.#saveProblem = `Tsugiki can't save the palette's recent commands: ${answer.error.message}`;
    }
  }

  /** `rows` in the palette's order: those run, most recently run first, then the others in their own
   * order. */
  order<Row extends Remembered>(rows: readonly Row[]): Row[] {
    const places = this.#places;
    const place = (row: Row) =>
      (row.history === false ? undefined : places.get(row.id ?? row.title)) ?? places.size;
    return rows.toSorted((one, other) => place(one) - place(other));
  }
}
