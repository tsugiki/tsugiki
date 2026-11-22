// A Repository's last sync and its open Issues and PRs as GitHub last counted them, open or not, which its
// page in Settings shows (#534). Asked for once, then again each time the core says a sync of that
// Repository finished, as a sync counts the open items on its way.
import { commands, events, type OpenItems, type RepositorySync_Serialize } from "$lib/data/bindings";
import { Followed } from "./followed.svelte";

export type RepositorySync = RepositorySync_Serialize;
export type { OpenItems };

export class RepositorySyncView extends Followed<RepositorySync> {
  constructor(repository: string) {
    super(
      () => commands.repositorySync(repository),
      [
        (again) =>
          events.lastSyncChanged.listen(({ payload }) => {
            if (payload.repository === repository) again();
          }),
      ],
    );
  }
}
