// Picking the clone a mirror borrows objects from (#172). The core opens the folder picker and checks the
// folder with git, which only reads it.
import { commands, type CloneRefusal, type MirrorCost, type MirrorSource } from "$lib/data/bindings";
import { ask } from "./answer";

export type { CloneRefusal, MirrorCost, MirrorSource };

/** The outcome of picking a clone for a Repository's mirror. */
export type PickedClone =
  | { kind: "cancelled" }
  | { kind: "lends"; folder: string }
  | { kind: "refused"; folder: string; refusal: CloneRefusal }
  | { kind: "failed"; problem: string };

/** Opens the folder picker, and checks the folder the user picks as a clone of `repository`. */
export async function pickClone(repository: string): Promise<PickedClone> {
  const picked = await ask(commands.pickFolder());
  if (picked.status === "error") return { kind: "failed", problem: picked.error.message };
  const folder = picked.data;
  if (folder === null) return { kind: "cancelled" };
  const checked = await ask(commands.checkClone(repository, folder));
  if (checked.status === "error") return { kind: "failed", problem: checked.error.message };
  const refusal = checked.data;
  return refusal === null ? { kind: "lends", folder } : { kind: "refused", folder, refusal };
}

/** What each Repository's mirror takes, as GitHub said, so each PR's Files tab doesn't ask again. */
const costs = new Map<string, MirrorCost>();

/** What `repository`'s mirror takes, as GitHub says, or nothing if Tsugiki couldn't ask. */
export async function mirrorCost(repository: string): Promise<MirrorCost | null> {
  const known = costs.get(repository);
  if (known !== undefined) return known;
  const answer = await ask(commands.mirrorCost(repository));
  if (answer.status === "error") return null;
  costs.set(repository, answer.data);
  return answer.data;
}

/** Saves where `repository`'s mirror gets its objects, and says what went wrong if it couldn't. */
export async function chooseMirrorSource(repository: string, source: MirrorSource) {
  const answer = await ask(commands.chooseMirrorSource(repository, source));
  return answer.status === "ok" ? undefined : answer.error.message;
}
