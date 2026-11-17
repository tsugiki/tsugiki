<!-- The dialog a click on the status bar's line opens, which asks where a Repository's mirror gets its
     objects, as the Files tab does (#311). It closes once the clone starts. -->
<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import MirrorQuestion from "./MirrorQuestion.svelte";

  let { repository, onclose }: { repository: string; onclose: () => void } = $props();
</script>

<Dialog label={`Mirror of ${repository}`} testid="mirror-question" size="wide" {onclose}>
  <h2>{`Mirror of ${repository}`}</h2>
  <MirrorQuestion {repository} onstarted={onclose} />
  <div class="buttons">
    <button onclick={onclose}>Close</button>
  </div>
</Dialog>

<style>
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
