// A Repository's state (#398): why Tsugiki can't write there, if it can't, and whether GitHub can be reached.
// Each write control reads it. Asked for once, then again each time the core says the Repository changed,
// as it does when the token's mark on it changes, and each time the status changes, which can say Tsugiki
// went offline or came back (ADR 0008).
import { commands, events, type Connection, type ReadOnly, type RepositoryState } from "$lib/data/bindings";
import { Followed, naming } from "./followed.svelte";

export type { Connection, ReadOnly, RepositoryState };

export class RepositoryStateView extends Followed<RepositoryState> {
  constructor(repository: string) {
    super(
      () => commands.repositoryState(repository),
      [naming(events.changed, repository), (again) => events.statusChanged.listen(again)],
    );
  }
}

/** Why Tsugiki can't write to `repository`, as each reason says it. */
const READ_ONLY: Record<ReadOnly, (repository: string) => string> = {
  token: (repository) => `your token can't write to ${repository}`,
};

/** The line atop the Issue and PR views of `repository` while it is Read-only, such as "Read-only: your
 * token can't write to python/cpython". Nothing, while Tsugiki can write there, or before the core says. */
export function readOnlyWords(state: RepositoryState | undefined, repository: string): string | null {
  if (state === undefined || state.read_only.length === 0) return null;
  const reasons = state.read_only.map((reason) => READ_ONLY[reason](repository));
  return `Read-only: ${reasons.join(", and ")}`;
}
