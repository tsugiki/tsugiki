// What the window opens on. The core decides, so this asks when the window opens, and the page gives it
// the answer to an add.
import { commands, type Home_Serialize, type TokenSource } from "$lib/data/bindings";

export type Home = Home_Serialize;
export type { TokenSource };

export class HomeState {
  current = $state<Home>();
  error = $state<string>();

  async load() {
    try {
      this.current = await commands.home();
    } catch (e) {
      this.error = String(e);
    }
  }
}
