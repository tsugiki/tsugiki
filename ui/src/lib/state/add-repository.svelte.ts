// The add dialog's steps: the user types a Repository, the core asks GitHub about it, and the user decides.
import {
  commands,
  type Conventions,
  type RepositoryCheck_Serialize,
  type RepositorySize,
} from "$lib/data/bindings";
import type { Home } from "./home.svelte";

export type RepositoryCheck = RepositoryCheck_Serialize;
export type { Conventions, RepositorySize };

export type AddStep =
  | { step: "typing" }
  | { step: "checking" }
  | { step: "checked"; check: RepositoryCheck }
  | { step: "adding"; check: RepositoryCheck }
  | { step: "failed"; problem: string };

type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: { message: string } };

/** The core's refusals come back as answers. Only a broken call rejects, and it reads the same. */
async function ask<T>(asking: Promise<Answer<T>>): Promise<Answer<T>> {
  try {
    return await asking;
  } catch (e) {
    return { status: "error", error: { message: String(e) } };
  }
}

export class AddRepositoryState {
  #input = $state("");
  step = $state<AddStep>({ step: "typing" });

  get input() {
    return this.#input;
  }

  /** A check says nothing about different text, so typing puts the dialog back to its first step. */
  set input(text: string) {
    this.#input = text;
    this.step = { step: "typing" };
  }

  async check() {
    const asked = this.#input;
    this.step = { step: "checking" };
    const answer = await ask(commands.checkRepository(asked));
    if (asked !== this.#input) {
      return;
    }
    this.step =
      answer.status === "ok"
        ? { step: "checked", check: answer.data }
        : { step: "failed", problem: answer.error.message };
  }

  /** Adds what was checked, by the name GitHub gave, and returns what the window opens on now. */
  async add(check: RepositoryCheck): Promise<Home | undefined> {
    this.step = { step: "adding", check };
    const answer = await ask(commands.addRepository(check.repository.name));
    if (answer.status === "ok") {
      return answer.data;
    }
    this.step = { step: "failed", problem: answer.error.message };
    return undefined;
  }
}
