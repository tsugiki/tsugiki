<!-- The dialog a click on the status bar's line opens, which asks where a Repository's mirror gets its
     objects, as the Files tab does (#311). It closes once the clone starts. -->
<script lang="ts">
  import { closeOnPress, takeFocus } from "./focus";
  import MirrorQuestion from "./MirrorQuestion.svelte";

  let { repository, onclose }: { repository: string; onclose: () => void } = $props();

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="mirror-question-scrim" onmousedown={closeOnPress(onclose)}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={`Mirror of ${repository}`}
    tabindex="-1"
    {@attach takeFocus}
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <h2>{`Mirror of ${repository}`}</h2>
    <MirrorQuestion {repository} onstarted={onclose} />
    <div class="buttons">
      <button onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
  }

  .dialog {
    width: min(640px, calc(100vw - 32px));
    margin: 60px auto 0;
    padding: 12px;
    box-sizing: border-box;
    font-size: 13px;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 30px var(--shadow);
  }

  .dialog:focus {
    outline: none;
  }

  h2 {
    margin: 0;
    font-size: 14px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
