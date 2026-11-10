// The question of where the mirror of a Repository Tsugiki has gets its objects, while its clone waits for
// the answer: in the Files tab, and in the dialog the status bar opens (#311).
import { MirrorChoosing } from "./mirror-choice.svelte";
import { chooseMirrorSource, mirrorCost, type MirrorCost } from "./mirror-source";

export class MirrorQuestionState {
  readonly repository: string;
  readonly mirror = new MirrorChoosing();
  /** What the mirror takes, or nothing until GitHub says, or if it can't. */
  cost = $state<MirrorCost | null>(null);
  /** Why the answer didn't take, until the next try. */
  problem = $state<string>();
  /** Whether the user's answer is on its way to the core, or the core took it. */
  answering = $state(false);

  constructor(repository: string) {
    this.repository = repository;
  }

  async load() {
    this.cost = await mirrorCost(this.repository);
  }

  /** Whether the user can start the clone: they chose a source, and haven't answered yet. */
  get canStart(): boolean {
    return this.mirror.source !== undefined && !this.answering;
  }

  /** Saves the user's answer, which starts the clone, and says whether the core took it. */
  async start(): Promise<boolean> {
    const source = this.mirror.source;
    if (source === undefined || this.answering) return false;
    this.answering = true;
    this.problem = await chooseMirrorSource(this.repository, source);
    this.answering = this.problem === undefined;
    return this.answering;
  }
}
