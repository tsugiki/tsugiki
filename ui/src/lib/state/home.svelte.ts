// What the window opens on. The core decides, so this asks when the window opens and on a switch, and the
// page gives it the answer to an add. Over the open Repository the window can show the Home page, which lists
// every Repository (#564): the core keeps the open one open, and leaving the page goes back to it.
import { commands, type Home_Serialize, type Opened, type TokenSource } from "$lib/data/bindings";
import { ask } from "./answer";

export type Home = Home_Serialize;
export type { Opened, TokenSource };

export class HomeState {
  current = $state<Home>();
  error = $state<string>();
  /** The problem that stopped the last switch, until a switch works. */
  switchProblem = $state<string>();
  /** Whether the Home page shows in place of the open Repository's screen. */
  page = $state(false);

  /** The Home page's rows: the open Repository, then the others, in the Repository picker's order, each
   * with when the user last opened it. None while no Repository is open. */
  get rows(): Opened[] {
    const home = this.current;
    if (home?.screen !== "repository") return [];
    return [{ name: home.repository.name, opened_at: home.opened_at }, ...home.others];
  }

  async load() {
    try {
      this.current = await commands.home();
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Shows the Home page, while a Repository is open. */
  goHome() {
    if (this.current?.screen === "repository") this.page = true;
  }

  /** Leaves the Home page for the open Repository, the one the user came from. */
  leave() {
    this.page = false;
  }

  /** Opens the Repository the user just added, which the core has made the open one. */
  added(next: Home) {
    this.current = next;
    this.switchProblem = undefined;
    this.page = false;
  }

  /** Switches to the Repository named `name`, leaving the Home page. The core saves it as the one opened
   * last and syncs it. A switch the core refuses leaves the window where it was. */
  async open(name: string) {
    const answer = await ask(commands.openRepository(name));
    if (answer.status === "ok") {
      this.current = answer.data;
      this.switchProblem = undefined;
      this.page = false;
    } else {
      this.switchProblem = `Tsugiki couldn't switch to ${name}: ${answer.error.message}`;
      await this.refreshOthers();
    }
  }

  /** The core read the files again to try a switch, so a Repository the user took out of them by hand leaves
   * the picker, the palette and the Home page. The window stays on its Repository, even if the core would
   * open another. */
  private async refreshOthers() {
    try {
      const now = await commands.home();
      const open = this.current;
      if (
        now.screen === "repository" &&
        open?.screen === "repository" &&
        now.repository.name === open.repository.name
      ) {
        this.current = { ...open, others: now.others };
      }
    } catch {
      // The list stays as it was, and the next switch or add brings it up to date.
    }
  }
}
