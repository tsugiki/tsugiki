// The note on a Repository picker's row that says why the Repository's diffs aren't there yet (#320).
import type { MirrorState } from "$lib/state/status.svelte";
import { cloneStage, OLDEST_GIT, sizeWords } from "./synced";

/** The note for `repository`'s mirror in `state`, or nothing for a mirror that is ready or that the core
 * hasn't heard of. */
export function mirrorNote(repository: string, state: MirrorState | undefined): string | null {
  if (state === undefined || state.state === "ready") return null;
  if (state.state === "cloning") {
    const { progress } = state;
    if (progress.stage === "starting") return "cloning";
    return `${cloneStage(progress).toLowerCase()}, ${String(progress.percent)}%`;
  }
  if (state.state === "asking") return "the mirror waits for your answer";
  if (state.state === "waiting") {
    return state.behind === null ? "waiting to clone" : `waiting for ${state.behind}'s clone`;
  }
  if (state.state === "offline") return "cloning starts once GitHub answers";
  // git can fail without a word, as when something kills it.
  if (state.state === "failed")
    return state.words === "" ? "the clone failed" : `the clone failed: ${state.words}`;
  const { problem } = state;
  if (problem.kind === "no_git") return "Tsugiki can't find git";
  if (problem.kind === "old_git") return `needs ${OLDEST_GIT} or newer: ${problem.version}`;
  // The disk is short for one clone, which holds back the others.
  if (problem.repository !== repository) return `no room on the disk for ${problem.repository}'s clone`;
  return `no room on the disk: it needs ${sizeWords(problem.needed_mb)}, ${sizeWords(problem.free_mb)} free`;
}
