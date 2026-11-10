// The user's choice of where a Repository's mirror gets its objects, in Add repository and wherever Tsugiki
// asks for a Repository it has (#172, #311).
import { pickClone, type MirrorSource, type PickedClone } from "./mirror-source";

/** The mirror's source of objects: all from GitHub, or borrowed from a clone the user picks. While the
 * picker is open nothing is picked yet. */
export type MirrorChoice = { kind: "download" } | { kind: "borrow"; picked: PickedClone | undefined };

export class MirrorChoosing {
  choice = $state<MirrorChoice>({ kind: "download" });

  /** The mirror's source of objects, or nothing while the user is still picking a clone that lends
   * them. */
  get source(): MirrorSource | undefined {
    if (this.choice.kind === "download") return { kind: "download" };
    const { picked } = this.choice;
    return picked?.kind === "lends" ? { kind: "borrow", folder: picked.folder } : undefined;
  }

  chooseDownload() {
    this.choice = { kind: "download" };
  }

  /** Opens the folder picker for a clone of `repository`. Cancelling keeps what was chosen before. */
  async chooseClone(repository: string) {
    const before = this.choice;
    this.choice = { kind: "borrow", picked: undefined };
    const picked = await pickClone(repository);
    this.choice = picked.kind === "cancelled" ? before : { kind: "borrow", picked };
  }
}
