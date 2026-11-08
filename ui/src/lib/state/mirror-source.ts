// Picking the clone a mirror borrows objects from (#172). The core opens the folder picker and checks the
// folder with git, which only reads it.
import { commands, type CloneRefusal, type MirrorSource } from "$lib/data/bindings";

export type { CloneRefusal, MirrorSource };

/** The outcome of picking a clone for a Repository's mirror. */
export type PickedClone =
  | { kind: "cancelled" }
  | { kind: "lends"; folder: string }
  | { kind: "refused"; folder: string; refusal: CloneRefusal }
  | { kind: "failed"; problem: string };

type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: { message: string } };

/** The core's refusals come back as answers. Only a broken call rejects, and it reads the same. */
async function ask<T>(asking: Promise<Answer<T>>): Promise<Answer<T>> {
  try {
    return await asking;
  } catch (e) {
    return { status: "error", error: { message: String(e) } };
  }
}

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

/** Saves where `repository`'s mirror gets its objects, and says what went wrong if it couldn't. */
export async function chooseMirrorSource(repository: string, source: MirrorSource) {
  const answer = await ask(commands.chooseMirrorSource(repository, source));
  return answer.status === "ok" ? undefined : answer.error.message;
}
