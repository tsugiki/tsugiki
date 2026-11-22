// A Repository's last sync and its open Issues and PRs as GitHub last counted them, open or not, which its
// page in Settings shows (#534). Asked for once, then again each time the core says that Repository changed,
// as it does when a sync saves a new count, and each time it says a sync of it finished.
import { commands, events, type OpenItems, type RepositorySync_Serialize } from "$lib/data/bindings";
import { Followed, naming } from "./followed.svelte";

export type RepositorySync = RepositorySync_Serialize;
export type { OpenItems };

export class RepositorySyncView extends Followed<RepositorySync> {
  constructor(repository: string) {
    super(
      () => commands.repositorySync(repository),
      [naming(events.changed, repository), naming(events.lastSyncChanged, repository)],
    );
  }
}
