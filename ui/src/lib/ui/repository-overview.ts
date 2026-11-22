// The line Settings › Repositories shows under each Repository (#533).
import type { OnThisMachine, RepositorySettings } from "$lib/state/settings-dialog.svelte";
import { sizeWords } from "./synced";

/** What Repository `repository` holds, in words: its Filters, whether its mirror borrows from a clone, and
 * how much of the disk its mirror takes, once the core has said what it has on this machine, `machine`. */
export function overviewLine(repository: RepositorySettings, machine: OnThisMachine | undefined) {
  const filters = repository.filters.length;
  const parts = [`${String(filters)} ${filters === 1 ? "Filter" : "Filters"}`];
  if (machine?.source?.kind === "borrow") parts.push("borrows from a clone");
  if (machine?.mirror.kind === "missing") parts.push("no mirror yet");
  if (machine?.mirror.kind === "measured") parts.push(`mirror ${mirrorSize(machine.mirror.mb)}`);
  return parts.join(" · ");
}

/** A mirror's size in whole megabytes, in words: a mirror smaller than one megabyte isn't empty. */
const mirrorSize = (mb: number) => (mb === 0 ? "under 1 MB" : sizeWords(mb));
