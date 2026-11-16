// What the core says about itself, asked for once when the window opens.
import { commands, type About } from "$lib/data/bindings";

export type { About };

/** Asks the core to open DevTools for this window. Only a development build has them (ADR 0013). */
export function openDevtools() {
  void commands.openDevtools();
}

export class AboutState {
  about = $state<About>();
  error = $state<string>();

  /** Tsugiki's name in the window's title, with the build's version once the core answers. */
  get title(): string {
    return this.about === undefined ? "Tsugiki" : `${this.about.name} ${this.about.version}`;
  }

  /** The status bar's words when the core gave no answer, with the reason. */
  get problem(): string | undefined {
    return this.error === undefined ? undefined : `The core didn't answer: ${this.error}`;
  }

  async load() {
    this.about = undefined;
    this.error = undefined;
    try {
      this.about = await commands.about();
    } catch (e) {
      this.error = String(e);
    }
  }
}
