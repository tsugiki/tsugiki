// What the window opens on. The core decides, so this asks when the window opens and on a switch, and the
// page gives it the answer to an add.
import { commands, type Home_Serialize, type TokenSource } from "$lib/data/bindings";
import { ask } from "./answer";

export type Home = Home_Serialize;
export type { TokenSource };

export class HomeState {
  current = $state<Home>();
  error = $state<string>();
  /** The problem that stopped the last switch, until a switch works. */
  switchProblem = $state<string>();

  async load() {
    try {
      this.current = await commands.home();
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Opens the Repository the user just added, which the core has made the open one. */
  added(next: Home) {
    this.current = next;
    this.switchProblem = undefined;
  }

  /** Switches to the Repository named `name`. The core saves it as the one opened last and syncs it. */
  async open(name: string) {
    const answer = await ask(commands.openRepository(name));
    if (answer.status === "ok") {
      this.current = answer.data;
      this.switchProblem = undefined;
    } else {
      this.switchProblem = `Tsugiki couldn't switch to ${name}: ${answer.error.message}`;
    }
  }
}
