<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";

  const state = new AboutState();
  void state.load();
</script>

<main>
  <AboutLine about={state.about} error={state.error} />
</main>
