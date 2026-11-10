<!-- PROTOTYPE, throwaway (#365): Go to definition to a file the PR doesn't change: the file, read-only, at
     the commit the symbol's line came from, over the PR's Files tab. Alt+← goes back to the diff where it
     was, and Alt+→ comes back here. -->
<script lang="ts">
  import { commitWords, fileKey, proto, type FileView } from "./definition-peek.svelte";
  import PeekCode from "./PeekCode.svelte";

  let { view, depth }: { view: FileView; depth: number } = $props();
  const backWords = $derived(depth > 1 ? "Back" : `Back to PR #${String(view.lookup.number)}'s diff`);
</script>

<div class="file-view" data-proto-file>
  <header>
    <button type="button" class="back" title="Alt+←" onclick={() => proto.back()}>← {backWords}</button>
    <span class="path">{view.path}</span>
    <span class="meta">at {commitWords(view.lookup)} of PR #{view.lookup.number}</span>
    <span class="badge">Read-only</span>
    <span class="spring"></span>
    <button
      type="button"
      onclick={() => proto.say(`Stub: moves the Review checkout to ${view.lookup.commit.slice(0, 7)} and runs code -g ${view.path}:${String(view.line)}`, 5000)}
      >Open in editor</button
    >
    <button
      type="button"
      onclick={() => {
        void navigator.clipboard.writeText(`${view.path}:${String(view.line)}`);
        proto.say("Copied the path and line.");
      }}>Copy path</button
    >
  </header>
  <p class="why">
    {`Go to definition of ${view.lookup.word} from ${view.lookup.from}:${String(view.lookup.fromLine)}.`}
    {#if view.changed}
      The PR changes this file, but not near line {view.line}, so it shows whole here.
    {:else}
      The PR doesn't change this file.
    {/if}
  </p>
  <div class="code">
    <PeekCode file={proto.files.get(fileKey(view.lookup.commit, view.path))} line={view.line} lead={8} />
  </div>
</div>

<style>
  .file-view {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    background: var(--proto-bg, Canvas);
    color: CanvasText;
    font-family: system-ui, sans-serif;
    font-size: 12px;
  }

  header {
    display: flex;
    flex: none;
    align-items: baseline;
    gap: 12px;
    padding: 4px 8px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 20%, transparent);
    background: color-mix(in srgb, currentColor 6%, transparent);
    white-space: nowrap;
  }

  .path {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-weight: 600;
  }

  .meta {
    opacity: 0.7;
  }

  .badge {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .spring {
    flex: 1;
  }

  button {
    padding: 0 6px;
    border: 0;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
    text-decoration: underline;
  }

  .back {
    text-decoration: none;
  }

  button:hover {
    background: color-mix(in srgb, #0969da 20%, transparent);
  }

  .why {
    flex: none;
    margin: 4px 8px;
    opacity: 0.7;
  }

  .code {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
