<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import StatusBar from "$lib/ui/StatusBar.svelte";

  const about = new AboutState();
  void about.load();
  const status = new StatusState();
  void status.load();
</script>

<main>
  <AboutLine about={about.about} error={about.error} />
</main>
<StatusBar status={status.status} error={status.error} />
