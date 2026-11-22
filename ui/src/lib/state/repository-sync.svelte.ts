// A Repository's last sync and its open Issues and PRs as GitHub last counted them, open or not, which its
// page in Settings shows (#534). Asked for once, then again each time the core says either changed for that
// Repository.
import { commands, events, type OpenItems, type RepositorySync_Serialize } from "$lib/data/bindings";
import { Followed, naming } from "./followed.svelte";

export type RepositorySync = RepositorySync_Serialize;
export type { OpenItems };

export class RepositorySyncView extends Followed<RepositorySync> {
  constructor(repository: string) {
    super(() => commands.repositorySync(repository), [naming(events.repositorySyncChanged, repository)]);
  }
}
