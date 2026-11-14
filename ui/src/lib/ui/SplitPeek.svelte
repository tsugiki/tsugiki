<!-- The code, the diff's or the file view's, with the split Definition peek below it (#459). The peek takes
     its place from the code, which stays where it was scrolled. Its top edge drags its height, from 42% of
     the room to start, and at least 160 px, and the pane layout saves it, as it saves the inline peek's rows.
     A peek with only words to say keeps its 2 rows and has no edge, as inline. The code stays drawn as the
     peek comes and goes, so its scroll and its focus stay. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { SPLIT_PEEK_LEAST, splitPeekMost } from "$lib/state/panes.svelte";
  import { peekPlaces, peekSized, type Peek } from "$lib/state/peek";
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

  const ROW = 20;
  /** The height of the code and the peek together. */
  let room = $state(0);
</script>

<div class="split" bind:clientHeight={room}>
  <div class="over">{@render children()}</div>
  {#if peek !== undefined}
    {@const sized = peekSized(peek)}
    {@const height = sized ? files.panes.splitPeekHeight(room) : peekPlaces(peek, 0) * ROW}
    <div class="under" style:height={`${String(height)}px`}>
      {#if sized}
        <!-- Astride the peek's top line, over the code's last pixels. -->
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
    position: relative;
    flex: none;
  }

  /* The strip the peek's top edge takes: 4 px of the code and 4 px of the peek. */
  .split-edge {
    position: absolute;
    top: -4px;
    z-index: 2;
    width: 100%;
    height: 8px;
  }
</style>
