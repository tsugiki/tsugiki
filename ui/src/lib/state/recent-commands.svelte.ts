// The command palette's commands the user ran, most recently run first, which the palette lists first
// (#556). The core keeps them in `machine.toml` beside the recent Repositories, so they survive a restart and
// stay on this machine.
import { commands } from "$lib/data/bindings";
import { ask } from "./answer";

/** A palette row as the history knows it: by its `id`, if it has one, or else by its title. */
export interface Remembered {
  title: string;
  /** What the history knows the row by where its title changes with what it does, as Hide Sidebar turns
   * into Show Sidebar: the action it runs. */
  id?: string;
}

export class RecentCommands {
  /** The ids of the commands run, most recently run first. */
  #ids = $state<string[]>([]);
  #problem = $state<string>();
  /** The last save, which the next one waits for, so the saves reach the file in the order the user ran
   * the commands. */
  #saving: Promise<void> = Promise.resolve();

  /** Why the history couldn't be saved, or read. */
  get problem(): string | undefined {
    return this.#problem;
  }

  /** Reads the history the core saved. A command run before the core answers stays ahead of what it read. */
  async load(): Promise<void> {
    try {
      const read = await commands.recentCommands();
      this.#ids = [...this.#ids, ...read.filter((id) => !this.#ids.includes(id))];
    } catch (e) {
      this.#problem = `Tsugiki can't read the palette's recent commands: ${String(e)}`;
    }
  }

  /** Puts `row` first, and saves that once the save before has finished. */
  ran(row: Remembered): Promise<void> {
    const id = row.id ?? row.title;
    this.#ids = [id, ...this.#ids.filter((one) => one !== id)];
    this.#saving = this.#saving.then(async () => {
      const answer = await ask(commands.rememberCommand(id));
      this.#problem =
        answer.status === "error"
          ? `Tsugiki can't save the palette's recent commands: ${answer.error.message}`
          : undefined;
    });
    return this.#saving;
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
