// Words for a clone the user picked for a mirror to borrow objects from.
import type { CloneRefusal, PickedClone } from "$lib/state/mirror-source";

/** Words for the reason `folder` can't lend its objects to `repository`'s mirror. */
export function refusalWords(folder: string, repository: string, refusal: CloneRefusal): string {
  if (refusal.kind === "not_a_folder") return `${folder} isn't a folder.`;
  if (refusal.kind === "not_a_clone") return `${folder} isn't a git clone: ${refusal.git_says}`;
  if (refusal.kind === "shallow") return `${folder} is a shallow clone, which git can't borrow from.`;
  return `${folder} has no remote for ${repository}.`;
}

/** Words for the outcome of picking a clone, or nothing when the user cancelled. */
export function pickedWords(picked: PickedClone, repository: string): string | null {
  if (picked.kind === "cancelled") return null;
  if (picked.kind === "lends") return `Borrows from ${picked.folder}`;
  if (picked.kind === "refused") return refusalWords(picked.folder, repository, picked.refusal);
  return picked.problem;
}
