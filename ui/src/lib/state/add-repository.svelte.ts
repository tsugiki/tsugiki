// The add dialog's steps: the user types a Repository, the core says Tsugiki has it or asks GitHub about it,
// and the user decides.
import {
  commands,
  type Conventions,
  type RepositoryCheck_Serialize,
  type RepositorySize,
} from "$lib/data/bindings";
import { ask } from "./answer";
import type { Home } from "./home.svelte";
import { MirrorChoosing } from "./mirror-choice.svelte";

export type RepositoryCheck = RepositoryCheck_Serialize;
export type { Conventions, RepositorySize };

export type AddStep =
  | { step: "typing" }
  | { step: "checking" }
  | { step: "checked"; check: RepositoryCheck }
  /** Tsugiki has the Repository already, by `name`, so the dialog offers to switch to it. */
  | { step: "held"; name: string }
  | { step: "adding"; check: RepositoryCheck }
  | { step: "failed"; problem: string };

export class AddRepositoryState {
  #input = $state("");
  step = $state<AddStep>({ step: "typing" });
  /** Where the mirror gets its objects. A clone picked after the text changed goes to the choice before. */
  mirror = $state(new MirrorChoosing());

  get input() {
    return this.#input;
  }

  /** A check says nothing about different text, so typing puts the dialog back to its first step. */
  set input(text: string) {
    this.#input = text;
    this.step = { step: "typing" };
    this.mirror = new MirrorChoosing();
  }

  async check() {
    const asked = this.#input;
    this.step = { step: "checking" };
    const answer = await ask(commands.checkRepository(asked));
    if (asked !== this.#input) {
      return;
    }
    if (answer.status === "error") {
      this.step = { step: "failed", problem: answer.error.message };
      return;
    }
    const found = answer.data;
    this.step =
      found.kind === "held" ? { step: "held", name: found.name } : { step: "checked", check: found.check };
  }

  /** Adds what was checked, by the name GitHub gave, and returns what the window opens on now. It adds
   * nothing while the user is still picking a clone, or the one they picked can't lend. */
  async add(check: RepositoryCheck): Promise<Home | undefined> {
    const source = this.mirror.source;
    if (source === undefined) return undefined;
    this.step = { step: "adding", check };
    const answer = await ask(
      commands.addRepository({ name: check.repository.name, access: check.access, source }),
    );
    if (answer.status === "ok") {
      return answer.data;
    }
    this.step = { step: "failed", problem: answer.error.message };
    return undefined;
  }
}
