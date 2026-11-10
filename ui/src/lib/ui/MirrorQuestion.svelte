<!-- Asks where the mirror of a Repository Tsugiki has gets its objects, while its clone waits for the answer,
     with the same two ways as Add repository (#311). The Files tab asks it, and so does the dialog the status
     bar opens. -->
<script lang="ts">
  import { MirrorQuestionState } from "$lib/state/mirror-question.svelte";
  import MirrorChoices from "./MirrorChoices.svelte";

  let { repository, onstarted = () => {} }: { repository: string; onstarted?: () => void } = $props();

  const question = $derived(new MirrorQuestionState(repository));
  $effect(() => {
    void question.load();
  });

  const start = async () => {
    if (await question.start()) onstarted();
  };
</script>

<div class="question">
  <p>
    {`Tsugiki shows the diff from its own copy of ${repository}, its mirror, which waits for you to say where it gets its objects.`}
  </p>
  <MirrorChoices {repository} cost={question.cost} choosing={question.mirror} disabled={question.answering} />
  {#if question.problem}
    <p role="alert">{question.problem}</p>
  {/if}
  <button disabled={!question.canStart} onclick={() => void start()}>Start the clone</button>
</div>

<style>
  .question {
    max-width: 640px;
    font-size: 13px;
  }

  p {
    margin: 8px 0 0;
    overflow-wrap: anywhere;
  }

  button {
    margin-top: 12px;
  }
</style>
