// The add dialog's steps: the user types a Repository, the core asks GitHub about it, and the user decides.
import {
  commands,
  type Conventions,
  type RepositoryCheck_Serialize,
  type RepositorySize,
} from "$lib/data/bindings";
import { ask } from "./answer";
import type { Home } from "./home.svelte";
import { pickClone, type MirrorSource, type PickedClone } from "./mirror-source";

export type RepositoryCheck = RepositoryCheck_Serialize;
export type { Conventions, RepositorySize };

export type AddStep =
  | { step: "typing" }
  | { step: "checking" }
  | { step: "checked"; check: RepositoryCheck }
  | { step: "adding"; check: RepositoryCheck }
  | { step: "failed"; problem: string };

/** The mirror's source of objects: all from GitHub, or borrowed from a clone the user picks. While the
 * picker is open nothing is picked yet. */
export type MirrorChoice = { kind: "download" } | { kind: "borrow"; picked: PickedClone | undefined };

const DOWNLOAD: MirrorChoice = { kind: "download" };

export class AddRepositoryState {
  #input = $state("");
  step = $state<AddStep>({ step: "typing" });
  mirror = $state<MirrorChoice>(DOWNLOAD);

  get input() {
    return this.#input;
  }

  /** A check says nothing about different text, so typing puts the dialog back to its first step. */
  set input(text: string) {
    this.#input = text;
    this.step = { step: "typing" };
    this.mirror = DOWNLOAD;
  }

  /** The mirror's source of objects, or nothing while the user is still picking a clone that lends
   * them. */
  get source(): MirrorSource | undefined {
    if (this.mirror.kind === "download") return { kind: "download" };
    const { picked } = this.mirror;
    return picked?.kind === "lends" ? { kind: "borrow", folder: picked.folder } : undefined;
  }

  chooseDownload() {
    this.mirror = DOWNLOAD;
  }

  /** Opens the folder picker for a clone of `repository`. Cancelling keeps what was chosen before. */
  async chooseClone(repository: string) {
    const before = this.mirror;
    this.mirror = { kind: "borrow", picked: undefined };
    const picked = await pickClone(repository);
    this.mirror = picked.kind === "cancelled" ? before : { kind: "borrow", picked };
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

  /** Adds what was checked, by the name GitHub gave, and returns what the window opens on now. It adds
   * nothing while the user is still picking a clone, or the one they picked can't lend. */
  async add(check: RepositoryCheck): Promise<Home | undefined> {
    const source = this.source;
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
