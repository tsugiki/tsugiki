<!-- The code, the diff's or the file view's, with the split Definition peek below it (#459). The peek takes
     its place from the code. Its top edge drags its height, from 42% of the room to start, and at least
     160 px, and the pane layout saves it, as it saves the inline peek's rows. The edge is a strip of its own
     between the two, so it covers neither the code's scrollbar nor the peek's header. A peek with only words
     to say keeps its 2 rows and has no edge, as inline. The code stays drawn as the peek comes and goes, so
     its scroll and its focus stay. The room is measured all along, so a peek opens at its height at once,
     and the code can keep the line it opened under in view in that same frame. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { SPLIT_PEEK_LEAST, splitPeekMost } from "$lib/state/panes.svelte";
  import { peekPlaces, peekSized, ROW, type Peek } from "$lib/state/peek";
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import PaneEdge from "./PaneEdge.svelte";
  import PeekBox from "./PeekBox.svelte";

  let {
    files,
    peek,
    children,
  }: {
    files: PrFilesState;
    /** The split peek below the code, while one shows there. */
    peek: Peek | undefined;
    children: Snippet;
  } = $props();

  /** The height of the code, the edge and the peek together. */
  let room = $state(0);
</script>

<div class="split" bind:clientHeight={room}>
  <div class="over">{@render children()}</div>
  {#if peek !== undefined}
    {@const sized = peekSized(peek)}
    {@const height = sized ? files.panes.splitPeekHeight(room) : peekPlaces(peek, 0) * ROW}
    {#if sized}
      <div class="split-edge">
        <PaneEdge
          label="Definition peek height"
          sizes={{ kind: "above", value: height }}
          least={SPLIT_PEEK_LEAST}
          most={splitPeekMost(room)}
          onresize={(size) => (files.panes.splitPeek = size)}
          ondone={() => void files.panes.save()}
          onreset={() => void files.panes.reset("splitPeek")}
        />
      </div>
    {/if}
    <div class="under" style:height={`${String(height)}px`}>
      <PeekBox diff={files} {peek} place={{ kind: "split" }} />
    </div>
  {/if}
</div>

<style>
  /* Fills what its parent leaves, the code above and the peek below. */
  .split {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  /* The code takes what the peek leaves, and lays itself out as it did without the peek. */
  .over {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .under {
    flex: none;
  }

  /* The strip the peek's top edge takes, between the code and the peek. */
  .split-edge {
    flex: none;
    height: 6px;
  }
</style>
