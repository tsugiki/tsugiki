// The facts under a Repository's name atop its page in Settings (#534).
import { counted } from "$lib/state/counted";
import type { RepositoryState } from "$lib/state/repository-state.svelte";
import type { OpenItems } from "$lib/state/repository-sync.svelte";

/** Whether Tsugiki can write to the Repository: Read-only for any reason, or nothing before the core says. */
export function writeWords(state: RepositoryState | undefined): string | null {
  if (state === undefined) return null;
  return state.read_only.length > 0 ? "Read-only" : "Can write";
}

/** Its open Issues and PRs as GitHub last counted them, such as "1,204 open Issues · 1 open PR". */
export function openWords(open: OpenItems): string {
  return `${counted(open.issues, "open Issue", "open Issues")} · ${counted(open.prs, "open PR", "open PRs")}`;
}
